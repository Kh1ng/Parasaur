//! Secret-free execution-node observations. They replace only coordinator-local
//! executable checks, never configured permissions, approvals or spend history.
use super::*;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCatalog {
    pub protocol: u32,
    pub node_id: String,
    pub profile: String,
    pub repo_id: String,
    pub repo: String,
    pub instances: Vec<WorkerInstance>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerInstance {
    pub backend_instance: String,
    pub logical_backend: String,
    pub enabled: bool,
    pub executable_ready: bool,
    pub auth_ready: Option<bool>,
    pub supported_models: Vec<String>,
}

impl WorkerCatalog {
    pub fn read(path: &Path) -> Result<Self> {
        read_json(&read_blob(path, None, MAX_JSON)?)
    }

    pub(crate) fn apply(
        &self,
        profile_name: &str,
        profile: &crate::config::Profile,
        node_id: &str,
        runtime: &mut crate::routing::RoutingRuntimeState,
    ) -> Result<()> {
        let bounded = |value: &str| {
            !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
        };
        ensure!(
            (1..=super::CATALOG_PROTOCOL).contains(&self.protocol)
                && self.node_id == node_id
                && bounded(node_id)
                && self.profile == profile_name
                && self.repo_id == profile.repo_id
                && self.repo == profile.repo
                && self.instances.len() <= 128,
            "Worker catalog identity or protocol changed"
        );
        let mut seen = HashSet::new();
        let mut available = HashMap::new();
        for instance in &self.instances {
            ensure!(
                bounded(&instance.backend_instance)
                    && bounded(&instance.logical_backend)
                    && seen.insert(instance.backend_instance.clone())
                    && instance.supported_models.len() <= 256
                    && instance.supported_models.iter().all(|model| bounded(model)),
                "Invalid worker catalog instance"
            );
            if instance.enabled && instance.executable_ready && instance.auth_ready != Some(false) {
                available.insert(
                    instance.backend_instance.clone(),
                    crate::routing::RemoteInstanceAvailability {
                        backend: instance.logical_backend.clone(),
                        models: instance.supported_models.clone(),
                    },
                );
            }
        }
        runtime.remote_instances = Some(available);
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerCheckScope {
    catalog: WorkerCatalog,
    held_work: Vec<crate::central_claims::NativeHeldWork>,
}

impl WorkerCheckScope {
    pub fn read(path: &Path) -> Result<Self> {
        read_json(&read_blob(path, None, MAX_JSON)?)
    }
    pub fn node_id(&self) -> &str {
        &self.catalog.node_id
    }
    pub fn check_current(
        &self,
        cfg: &GahConfig,
        context: &Path,
        expected: &str,
        origin: &str,
        require_next_action: bool,
    ) -> Result<DispatchBinding> {
        super::work::check_current(
            cfg,
            context,
            expected,
            &self.catalog.node_id,
            origin,
            require_next_action,
            Some(self),
        )
    }
    pub(super) fn catalog(&self) -> &WorkerCatalog {
        &self.catalog
    }
    pub(super) fn project(
        &self,
        profile: &str,
        bound: &DispatchBinding,
        snapshot: Option<&mut crate::status::StatusSnapshot>,
    ) -> Result<()> {
        ensure!(
            bound.mode != "pm" || self.catalog.protocol >= 2,
            "Worker cannot return a native plan artifact"
        );
        ensure!(
            self.held_work.iter().any(|hold| hold.profile == profile
                && hold.run_id == bound.run_id
                && hold.node_id == bound.node_id
                && hold.context_sha256 == bound.context_sha256
                && crate::work_claim::normalize_work_identity(&hold.work_id)
                    == crate::work_claim::normalize_work_identity(&bound.work_id)),
            "Original native ownership is absent from central scope"
        );
        crate::controller::project_native_holds(
            profile,
            snapshot,
            &self.held_work,
            Some(&bound.run_id),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn catalog(cfg: &GahConfig) -> WorkerCatalog {
        let profile = &cfg.profiles["test"];
        WorkerCatalog {
            protocol: 1,
            node_id: "worker".into(),
            profile: "test".into(),
            repo_id: profile.repo_id.clone(),
            repo: profile.repo.clone(),
            instances: vec![WorkerInstance {
                backend_instance: "work".into(),
                logical_backend: "codex".into(),
                enabled: true,
                executable_ready: true,
                auth_ready: None,
                supported_models: vec!["fixture-model".into()],
            }],
        }
    }
    #[test]
    fn native_plan_uses_the_workers_account_without_a_coordinator_executable() {
        let _exec = crate::test_support::ExecGuard::new();
        let (directory, mut cfg, _worker, mut request) = super::super::tests::fixture();
        let _availability = crate::test_support::AvailabilityEnvGuard::set(
            directory.path().join("availability.json"),
        );
        let profile = cfg.profiles.get_mut("test").unwrap();
        profile.routing.improve_candidates.as_mut().unwrap()[0].instance = Some("work".into());
        request.args.backend = "auto".into();
        request.args.backend_instance = None;
        request.args.model = None;
        for mode in ["fix", "improve"] {
            request.args.mode = mode.into();
            let planned = crate::dispatch::plan_native(&cfg, &request, &catalog(&cfg)).unwrap();
            assert_eq!(planned.args.mode, mode);
            assert_eq!(planned.args.backend, "codex");
            assert_eq!(planned.args.backend_instance.as_deref(), Some("work"));
            assert_eq!(planned.args.model.as_deref(), Some("fixture-model"));
            assert_eq!(planned.args.run_id, request.args.run_id);
        }
    }
    /// A fix that finished and passed validation on the worker but never got
    /// central's permission to publish, and the publication retry planned
    /// for it. The worker offers no model: publishing needs none.
    fn unpublished_fix() -> (
        tempfile::TempDir,
        GahConfig,
        crate::ledger::LedgerEntry,
        JobRequest,
    ) {
        use crate::ledger::remote_dispatch::{
            ProcessTermination, ReceivedExecution, TerminationReason,
        };
        let (dir, mut cfg, _worker, mut request) = super::super::tests::fixture();
        let profile = cfg.profiles.get_mut("test").unwrap();
        profile.provider = "gitlab".into();
        profile.publishing.allow_pull_request_creation = true;
        profile.publishing.allow_commit_message_generation = true;
        request.args.target = "1525".into();
        let source_id = uuid::Uuid::new_v4().to_string();
        let mut entry = crate::ledger::LedgerEntry::new(
            "test",
            profile,
            "codex",
            "fix",
            "1525",
            Some(source_id.clone()),
            None,
        );
        entry.work_id = request.args.work_id.clone();
        entry.source_issue_number = Some("1525".into());
        entry.dispatch_reason = Some("initial".into());
        entry.commit_created = true;
        entry.validation_result = Some("passed".into());
        entry.failure_class = Some("harness_error".into());
        entry.error_summary = Some("Native operation authority is held: clock".into());
        entry.effective_model = request.args.model.clone();
        entry.branch = Some("gah/retained".into());
        entry.remote_execution = Some(ReceivedExecution {
            node_id: "worker".into(),
            context_sha256: "1".repeat(64),
            receipt_sha256: "2".repeat(64),
            termination: ProcessTermination {
                reason: TerminationReason::Exited,
                exit_code: Some(1),
                signal: None,
            },
        });
        crate::ledger::append(&cfg, &entry).unwrap();
        let mut worker = catalog(&cfg);
        worker.instances.clear();
        let planned = crate::dispatch::plan_native(&cfg, &request, &worker).unwrap();
        assert_eq!(planned.publication_source.as_ref(), Some(&source_id));
        (dir, cfg, entry, planned)
    }
    #[test]
    fn publication_retry_requires_the_acknowledged_issue_and_worker_even_without_model_capacity() {
        let _guard = crate::test_support::ExecGuard::new();
        let (_dir, cfg, entry, planned) = unpublished_fix();
        let profile = &cfg.profiles["test"];
        let verify = |req: &JobRequest, worker: &str, entries: &[crate::ledger::LedgerEntry]| {
            crate::dispatch::publication_recovery::verify(req, entries, profile, worker).is_ok()
        };
        assert!(verify(&planned, "worker", std::slice::from_ref(&entry)));
        let mut child = planned.clone();
        child.args.mode = "improve".into();
        assert!(!verify(&child, "worker", std::slice::from_ref(&entry)));
        let mut child_entry = entry.clone();
        child_entry.mode = "improve".into();
        assert!(verify(&child, "worker", &[child_entry]));
        assert!(!verify(
            &planned,
            "another-worker",
            std::slice::from_ref(&entry)
        ));
        let mut changed = planned.clone();
        changed.publication_source = Some(uuid::Uuid::new_v4().to_string());
        assert!(!verify(&changed, "worker", std::slice::from_ref(&entry)));
        let mut newer = entry.clone();
        newer.mr_created = true;
        assert!(!verify(&planned, "worker", &[entry, newer]));
    }
    #[test]
    fn a_publication_retry_refused_before_publishing_is_not_offered_again() {
        let _guard = crate::test_support::ExecGuard::new();
        let (_dir, cfg, entry, planned) = unpublished_fix();
        let profile = &cfg.profiles["test"];
        type Entry = crate::ledger::LedgerEntry;
        let offered = |implementation: &Entry, earlier: &[&Entry]| {
            let mut history = vec![implementation.clone()];
            history.extend(earlier.iter().map(|&retry| retry.clone()));
            crate::dispatch::publication_recovery::verify(&planned, &history, profile, "worker")
                .is_ok()
        };
        let offered_after = |earlier: &[&Entry]| offered(&entry, earlier);
        // An earlier publication retry of the same fix. Its job records the
        // issue, the branch and the validation result only once it has
        // revalidated the preserved commit.
        let retry = |error: &str, revalidated: bool| {
            let mut retry = entry.clone();
            retry.session_id = Some(uuid::Uuid::new_v4().to_string());
            retry.dispatch_reason = Some("recover_publication".into());
            retry.commit_created = false;
            retry.error_summary = Some(error.into());
            if !revalidated {
                retry.source_issue_number = None;
                retry.branch = None;
                retry.validation_result = None;
            }
            retry
        };

        // Other work merged first, so the preserved commit no longer sits on
        // the target. Repeating the same retry cannot change that; it used to
        // be offered until the issue was parked for an operator.
        let target_moved = retry(
            "Preserved implementation needs validation against a changed target",
            false,
        );
        assert!(!offered_after(&[&target_moved]));

        // Central's permission did not arrive this time either: nothing was
        // decided about the commit, so it is tried again.
        let held = retry("Native operation authority is held", true);
        assert!(offered_after(&[&held]));
        assert!(!offered_after(&[&held, &target_moved]));

        // A retry pushed the branch and only the merge request is missing.
        // What remains is safe to repeat and must not be redone from
        // scratch, which would leave that branch on the remote with no
        // merge request.
        let mut pushed = retry("Provider request failed", true);
        pushed.push_attempted = true;
        pushed.push_succeeded = true;
        pushed.mr_attempted = true;
        assert!(offered_after(&[&pushed]));
        // That holds when a later retry then fails before it pushes again,
        // for whatever reason.
        let lookup_failed = retry("Provider request failed", false);
        assert!(offered_after(&[&pushed, &lookup_failed]));
        assert!(offered_after(&[&pushed, &target_moved]));
        assert!(offered_after(&[&target_moved, &pushed]));

        // The same when the push was the implementation's own: it pushed,
        // could not create the merge request, and kept its commit as a
        // checkpoint.
        let mut pushed_itself = entry.clone();
        pushed_itself.error_summary = Some("Provider request failed".into());
        pushed_itself.push_attempted = true;
        pushed_itself.push_succeeded = true;
        pushed_itself.mr_attempted = true;
        pushed_itself.resumable_checkpoint_sha = Some("3".repeat(40));
        pushed_itself.resumable_checkpoint_branch = pushed_itself.branch.clone();
        assert!(offered(&pushed_itself, &[]));
        assert!(offered(&pushed_itself, &[&lookup_failed]));
        assert!(offered(&pushed_itself, &[&target_moved]));
        // A push that was attempted and failed counts too: the ledger does
        // not say how far it got.
        pushed_itself.push_succeeded = false;
        pushed_itself.mr_attempted = false;
        assert!(offered(&pushed_itself, &[&target_moved]));
    }
    #[test]
    fn unavailable_worker_accounts_cannot_borrow_a_coordinators_executable_or_other_model() {
        let _exec = crate::test_support::ExecGuard::new();
        let (directory, mut cfg, _worker, request) = super::super::tests::fixture();
        let _availability = crate::test_support::AvailabilityEnvGuard::set(
            directory.path().join("availability.json"),
        );
        let executable = directory.path().join("provider");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        cfg.profiles
            .get_mut("test")
            .unwrap()
            .routing
            .backend_instances
            .get_mut("work")
            .unwrap()
            .executable = Some(executable.to_string_lossy().into_owned());
        for change in 0..4 {
            let mut unavailable = catalog(&cfg);
            match change {
                0 => unavailable.instances[0].enabled = false,
                1 => unavailable.instances[0].executable_ready = false,
                2 => unavailable.instances[0].auth_ready = Some(false),
                _ => unavailable.instances[0].supported_models = vec!["other-model".into()],
            }
            assert!(
                crate::dispatch::plan_native(&cfg, &request, &unavailable).is_err(),
                "accepted unavailable account case {change}"
            );
        }
    }
    #[test]
    fn old_workers_cannot_accept_native_decomposition() {
        let (_directory, cfg, _worker, mut request) = super::super::tests::fixture();
        request.args.mode = "pm".into();
        request.args.retries = 0;
        request.args.dispatch_reason = Some("pm_decomposition".into());
        let error = crate::dispatch::plan_native(&cfg, &request, &catalog(&cfg))
            .err()
            .unwrap();
        assert!(error
            .to_string()
            .contains("cannot return a native plan artifact"));
    }
    #[test]
    fn catalog_identity_and_canonical_ownership_scope_are_independent_requirements() {
        let (_directory, cfg, _worker, request) = super::super::tests::fixture();
        let mut wrong = catalog(&cfg);
        wrong.repo = "other/repo".into();
        assert!(wrong
            .apply(
                "test",
                &cfg.profiles["test"],
                "worker",
                &mut Default::default()
            )
            .is_err());
        let portable = super::super::policy::prepare(&cfg, "test").unwrap();
        let bound = super::super::binding(&request, "worker", &portable, &"a".repeat(64)).unwrap();
        let mut scope = WorkerCheckScope {
            catalog: catalog(&cfg),
            held_work: vec![],
        };
        assert!(scope.project("test", &bound, None).is_err());
        scope.held_work.push(crate::central_claims::NativeHeldWork {
            run_id: bound.run_id.clone(),
            node_id: bound.node_id.clone(),
            context_sha256: bound.context_sha256.clone(),
            profile: "test".into(),
            work_id: bound.work_id.clone(),
            reserves_intake: true,
        });
        scope.project("test", &bound, None).unwrap();
        scope.held_work[0].context_sha256 = "b".repeat(64);
        assert!(scope.project("test", &bound, None).is_err());
    }
}
