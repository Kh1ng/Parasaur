//! Authority for one dedicated native job process. Installation is one-way and
//! requires its consumed start and actual service identity. Ordinary CLI/server
//! processes cannot opt in through serialized dispatch arguments or ambient env.
use super::{
    worker::{OperationPermission, OperationPurpose, WorkerRun},
    WorkerContext,
};
use crate::{
    config::{GahConfig, Profile},
    dispatch::DispatchArgs,
    execution_identity::ExecutionIdentity,
    runner,
};
use anyhow::{ensure, Context, Result};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    time::{Duration, Instant},
};

struct Execution {
    context: WorkerContext,
    work: PathBuf,
    token: String,
    unconfirmed: AtomicBool,
}
static CURRENT: OnceLock<Execution> = OnceLock::new();
thread_local! {static PUBLICATION:Cell<bool>=const{Cell::new(false)};}

/// Install only in the fixed job wrapper after consuming central's grant.
/// No backend receives the service token through args, environment or files.
pub fn install(context: WorkerContext, work: PathBuf, token: String) -> Result<()> {
    ensure!(
        !token.is_empty() && token.len() <= 4096 && !token.chars().any(char::is_control),
        "Native service credential is unavailable"
    );
    ensure!(
        context.request.args.route_admission.is_none(),
        "Native jobs already own their host admission"
    );
    WorkerRun::open(&work, &context.binding)?.require_execution()?;
    CURRENT
        .set(Execution {
            context,
            work,
            token,
            unconfirmed: AtomicBool::new(false),
        })
        .map_err(|_| anyhow::anyhow!("Native process authority is already installed"))
}

pub(crate) fn active() -> bool {
    CURRENT.get().is_some()
}

pub(crate) fn verify_environment() -> Result<()> {
    if let Some(execution) = CURRENT.get() {
        super::verify_history_environment(&execution.work)?;
    }
    Ok(())
}

pub(crate) fn verify_dispatch(cfg: &GahConfig, args: &DispatchArgs) -> Result<()> {
    if let Some(execution) = CURRENT.get() {
        ensure!(
            serde_json::to_value(cfg)? == serde_json::to_value(&execution.context.config)?
                && serde_json::to_value(args)?
                    == serde_json::to_value(&execution.context.request.args)?,
            "Native dispatch changed its verified configuration or request"
        );
        WorkerRun::open(&execution.work, &execution.context.binding)?.require_execution()?;
    }
    Ok(())
}

impl Execution {
    fn verify_identity(&self, identity: &ExecutionIdentity) -> Result<()> {
        let bound = &self.context.binding;
        ensure!(
            identity.logical_backend == bound.backend
                && identity.effective_model == bound.model
                && bound
                    .backend_instance
                    .as_deref()
                    .is_none_or(|id| id == identity.backend_instance),
            "Native invocation changed its selected account or model"
        );
        Ok(())
    }

    fn verify_profile(&self, profile: &Profile) -> Result<()> {
        let expected =
            crate::config::get_profile(&self.context.config, &self.context.binding.profile)?;
        ensure!(
            serde_json::to_value(profile)? == serde_json::to_value(expected)?,
            "Native publication changed its verified profile"
        );
        Ok(())
    }
    fn begin(
        &self,
        purpose: OperationPurpose,
        identity: Option<&ExecutionIdentity>,
        profile: Option<&Profile>,
    ) -> Result<OperationPermission> {
        ensure!(
            !self.unconfirmed.load(Ordering::SeqCst),
            "Native operation completion is unconfirmed; stop and reconcile this job"
        );
        if let Some(identity) = identity {
            self.verify_identity(identity)?;
        }
        if let Some(profile) = profile {
            self.verify_profile(profile)?;
        }
        WorkerRun::open(&self.work, &self.context.binding)?.authorize_central(
            &self.context,
            purpose,
            &self.token,
        )
    }

    fn finish(&self, permission: &OperationPermission) -> Result<()> {
        ensure!(
            !self.unconfirmed.load(Ordering::SeqCst),
            "Native operation completion is unconfirmed; retain its operation fence"
        );
        // Neither a direct child's exit nor successful log parsing proves that
        // no orphan backend is still using this job's resources.
        quiescent(&self.context.binding.run_id)?;
        WorkerRun::open(&self.work, &self.context.binding)?.complete_operation(permission)
    }
}

fn quiescent(run_id: &str) -> Result<()> {
    let group = std::fs::read_to_string("/proc/self/cgroup")?;
    let path = group
        .strip_prefix("0::")
        .and_then(|value| value.strip_suffix('\n'))
        .filter(|value| {
            value.starts_with('/')
                && !value.contains('\n')
                && value.ends_with(&format!("/gah-native-job-{run_id}.service"))
        })
        .context("Native execution left its job cgroup")?;
    let members = Path::new("/sys/fs/cgroup")
        .join(path.trim_start_matches('/'))
        .join("cgroup.procs");
    let current = std::process::id().to_string();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let values = std::fs::read_to_string(&members)?;
        if values.split_whitespace().all(|pid| pid == current)
            && values.split_whitespace().any(|pid| pid == current)
        {
            return Ok(());
        }
        ensure!(
            Instant::now() < deadline,
            "Native backend descendants remain; completion is unconfirmed"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn operate<T>(
    execution: &Execution,
    prepare: impl FnOnce() -> Result<OperationPermission>,
    invoke: impl FnOnce() -> Result<T>,
    confirmed: impl FnOnce(&T) -> bool,
) -> Result<T> {
    let permission =
        prepare().inspect_err(|_| execution.unconfirmed.store(true, Ordering::SeqCst))?;
    let result = invoke();
    let complete = result.as_ref().is_ok_and(confirmed) && execution.finish(&permission).is_ok();
    if !complete {
        execution.unconfirmed.store(true, Ordering::SeqCst);
        eprintln!("Native operation remains held; no further backend or publication is permitted in this job");
    }
    // Preserve actual exit/usage evidence even if completion could not be
    // recorded. The final dispatch check prevents a false successful job.
    result
}

pub(crate) fn attempt(
    identity: &ExecutionIdentity,
    invoke: impl FnOnce() -> Result<runner::RunResult>,
) -> Result<runner::RunResult> {
    match CURRENT.get() {
        None => invoke(),
        Some(execution) => operate(
            execution,
            || execution.begin(OperationPurpose::Attempt, Some(identity), None),
            invoke,
            |result| result.exit_code != runner::process::PROCESS_CLEANUP_FAILED_EXIT_CODE,
        ),
    }
}

pub(crate) fn review(
    identity: &ExecutionIdentity,
    invoke: impl FnOnce() -> Result<runner::ReviewRunResult>,
) -> Result<runner::ReviewRunResult> {
    match CURRENT.get() {
        None => invoke(),
        Some(execution) => operate(
            execution,
            || execution.begin(OperationPurpose::Attempt, Some(identity), None),
            invoke,
            |result| {
                !matches!(
                    result.outcome,
                    runner::ReviewProcessOutcome::CleanupFailure(_)
                )
            },
        ),
    }
}

pub(crate) fn publication<T>(profile: &Profile, invoke: impl FnOnce() -> Result<T>) -> Result<T> {
    publication_observed(profile, invoke, |_| true)
}

pub(crate) fn publication_observed<T>(
    profile: &Profile,
    invoke: impl FnOnce() -> Result<T>,
    confirmed: impl FnOnce(&T) -> bool,
) -> Result<T> {
    let Some(execution) = CURRENT.get() else {
        return invoke();
    };
    // One logical publication may call multiple provider helpers. Reuse only
    // this thread's active publication; other threads still need a new fence.
    execution
        .verify_profile(profile)
        .inspect_err(|_| execution.unconfirmed.store(true, Ordering::SeqCst))?;
    if PUBLICATION.get() {
        let result = invoke();
        if !result.as_ref().is_ok_and(confirmed) {
            execution.unconfirmed.store(true, Ordering::SeqCst);
        }
        return result;
    }
    let permission = || execution.begin(OperationPurpose::Publish, None, Some(profile));
    operate(
        execution,
        permission,
        || {
            struct Reset;
            impl Drop for Reset {
                fn drop(&mut self) {
                    PUBLICATION.set(false)
                }
            }
            PUBLICATION.set(true);
            let _reset = Reset;
            invoke()
        },
        confirmed,
    )
}

pub(crate) fn verify_identity(identity: &ExecutionIdentity) -> Result<()> {
    if let Some(execution) = CURRENT.get() {
        execution
            .verify_identity(identity)
            .inspect_err(|_| execution.unconfirmed.store(true, Ordering::SeqCst))?;
    }
    Ok(())
}

pub(crate) fn finish_dispatch() -> Result<()> {
    if let Some(execution) = CURRENT.get() {
        ensure!(
            !execution.unconfirmed.load(Ordering::SeqCst),
            "Native operation remains unconfirmed; preserve this job for reconciliation"
        );
        WorkerRun::open(&execution.work, &execution.context.binding)?.require_execution()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
