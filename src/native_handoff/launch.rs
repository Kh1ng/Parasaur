//! One fixed transient job per durable start offer. Credential bytes stay in
//! memory and a PID-verified socket, never a unit property, argument or file.
use super::{host, worker::WorkerRun, wrapper};
use anyhow::{ensure, Context, Result};
use std::{
    fs::OpenOptions,
    os::unix::{fs::OpenOptionsExt, net::UnixListener},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn path(value: &Path) -> Result<PathBuf> {
    let canonical = value.canonicalize()?;
    let text = canonical
        .to_str()
        .context("Native launch path is not UTF-8")?;
    // systemd expands unit specifiers. Refuse ambiguous paths before fencing a
    // start, rather than risk delivering credentials/output to another path.
    ensure!(
        !text.contains('%') && !text.chars().any(char::is_control),
        "Native launch path contains unsupported characters"
    );
    Ok(canonical)
}

fn start_job(
    context: &Path,
    expected: &str,
    work: &Path,
    config: Option<&Path>,
    socket: &Path,
    run_id: &str,
) -> Result<host::JobIdentity> {
    // Logs are private, exclusive evidence for this one job. Never follow an
    // old log symlink or silently truncate evidence from an earlier launch.
    for name in ["job.stdout", "job.stderr"] {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(work.join(name))?;
    }
    let mut command = Command::new("/usr/bin/systemd-run");
    command
        .args([
            "--user",
            "--quiet",
            "--no-ask-password",
            "--service-type=exec",
            "--expand-environment=no",
        ])
        .arg(format!("--unit=gah-native-job-{run_id}.service"));
    for property in ["ExitType=cgroup", "RemainAfterExit=yes", "Restart=no", "KillMode=control-group",
        "StopWhenUnneeded=no", "UMask=0077", "TimeoutStartSec=5s",
        "UnsetEnvironment=GAH_LEDGER_PATH GAH_EVENTS_PATH GAH_RECONCILIATION_PATH COORDINATOR_TOKEN"] {
        command.args(["-p", property]);
    }
    command
        .arg("-p")
        .arg("StandardInput=null")
        .arg("-p")
        .arg(format!(
            "StandardOutput=file:{}",
            work.join("job.stdout").display()
        ))
        .arg("-p")
        .arg(format!(
            "StandardError=file:{}",
            work.join("job.stderr").display()
        ));
    // Retain only node-local lookup context. The owner credential is never an
    // environment property and the manager supplies the job's own invocation.
    for key in ["HOME", "PATH", "GAH_COORDINATOR_IDENTITY_PATH"] {
        if let Some(value) = std::env::var_os(key) {
            let value = value
                .to_str()
                .context("Native lookup environment is not UTF-8")?;
            ensure!(
                !value.chars().any(char::is_control),
                "Native lookup environment is invalid"
            );
            command.arg(format!("--setenv={key}={value}"));
        }
    }
    command
        .arg(path(&std::env::current_exe()?)?)
        .args(["native-handoff", "execute", "--context-dir"])
        .arg(context)
        .args(["--expected-context", expected, "--work-dir"])
        .arg(work)
        .arg("--credential-socket")
        .arg(socket);
    if let Some(config) = config {
        command.arg("--config-path").arg(config);
    }
    let response = crate::runner::process::run_bounded(command, Duration::from_secs(3))
        .context("Native launch reply unavailable; reconcile this start")?;
    ensure!(
        response.status.success(),
        "Native launch was not confirmed; reconcile this start"
    );
    host::observe_job(run_id)
}

/// Offer exactly one verified wrapper, pin its observed process identity, then
/// release the writer lock before delivering its credential envelope. A lost
/// reply or failed delivery preserves StartIntent and capacity, never retries.
pub fn run(
    context_dir: &Path,
    expected: &str,
    work: &Path,
    config: Option<&str>,
) -> Result<serde_json::Value> {
    let credentials = wrapper::receive_input()?;
    let context_dir = path(context_dir)?;
    let work = path(work)?;
    let config = config.map(Path::new).map(path).transpose()?;
    let context = wrapper::prepare(
        &context_dir,
        expected,
        &work,
        config.as_ref().and_then(|value| value.to_str()),
    )?;
    let payload = serde_json::to_vec(
        &serde_json::json!({"grant":&credentials.grant, "service_token":&credentials.service_token}),
    )?;
    ensure!(
        payload.len() <= 16 * 1024,
        "Native forwarded credential input exceeds its limit"
    );
    let runtime = PathBuf::from(
        std::env::var_os("XDG_RUNTIME_DIR")
            .context("Native launch requires its user runtime directory")?,
    );
    super::private_directory(&runtime)?;
    let runtime = path(&runtime)?;
    let temporary = tempfile::Builder::new()
        .prefix(".native-start-")
        .tempdir_in(runtime)?;
    let socket = temporary.path().join("input");
    let listener = UnixListener::bind(&socket)?;
    let mut journal = WorkerRun::open(&work, &context.binding)?;
    let mut identity = None;
    journal.offer_start(&credentials.grant, || {
        identity = Some(start_job(
            &context_dir,
            expected,
            &work,
            config.as_deref(),
            &socket,
            &context.binding.run_id,
        )?);
        Ok(())
    })?;
    let identity = identity.context("Native launch has no observed identity")?;
    journal.record_launch(identity.clone())?;
    drop(journal);
    super::credential_socket::deliver(&listener, identity.pid, &payload, || {
        host::require_job(
            &context.binding.run_id,
            &identity.invocation,
            identity.pid,
            &identity.start_ticks,
        )
    })?;
    Ok(
        serde_json::json!({"binding":context.binding, "start_offered":true,
        "launch_confirmed":true, "execution_authorized":false}),
    )
}
