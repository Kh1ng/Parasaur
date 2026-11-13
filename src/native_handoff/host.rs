//! Host admission for native jobs. A separate active/exited systemd service
//! retains the ordinary capacity lock without a resident broker process.
//! Reserving capacity never authorizes backend execution or publication.
use super::WorkerContext;
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap, os::fd::AsRawFd, os::unix::process::CommandExt, process::Command,
    time::Duration,
};
mod release;
pub(super) use release::{
    record_lease, release_capacity, release_unused_capacity, released_capacity,
};

fn state(unit: &str) -> Result<BTreeMap<String, String>> {
    let mut command = Command::new("/usr/bin/systemctl");
    command.args(["--user", "show", unit, "--property=Id,LoadState,ActiveState,SubState,MainPID,ControlGroup,InvocationID,Transient,Type,ExitType,RemainAfterExit,FileDescriptorStoreMax,NFileDescriptorStore,FileDescriptorStorePreserve,NotifyAccess,Restart,KillMode,StopWhenUnneeded,ExecMainPID,ExecMainCode,ExecMainStatus,ExecMainExitTimestampMonotonic,Slice"]);
    let output = crate::runner::process::run_bounded(command, Duration::from_secs(2))
        .context("Native capacity holder state is unavailable")?;
    ensure!(
        output.status.success() && output.stdout.len() <= 16384,
        "Native capacity holder state is unavailable"
    );
    let mut values = BTreeMap::new();
    for line in std::str::from_utf8(&output.stdout)?.lines() {
        let (key, value) = line
            .split_once('=')
            .context("Invalid capacity holder state")?;
        ensure!(
            values.insert(key.to_owned(), value.to_owned()).is_none(),
            "Duplicate capacity holder property"
        );
    }
    Ok(values)
}

fn validate_holder(
    values: &BTreeMap<String, String>,
    unit: &str,
    invocation: &str,
    own_cgroup: &str,
    stored: bool,
) -> Result<()> {
    validate_contract(values, unit, invocation)?;
    for (key, expected) in [
        ("ActiveState", "activating"),
        ("SubState", "start"),
        ("ControlGroup", own_cgroup),
        ("NFileDescriptorStore", if stored { "1" } else { "0" }),
    ] {
        ensure!(
            values.get(key).map(String::as_str) == Some(expected),
            "Native capacity holder has an unsafe or changed {key}"
        );
    }
    ensure!(
        values.get("MainPID") == Some(&std::process::id().to_string()),
        "Native capacity holder does not own this process"
    );
    Ok(())
}

fn validate_contract(
    values: &BTreeMap<String, String>,
    unit: &str,
    invocation: &str,
) -> Result<()> {
    for (key, expected) in [
        ("Id", unit),
        ("LoadState", "loaded"),
        ("Transient", "yes"),
        ("Type", "oneshot"),
        ("RemainAfterExit", "yes"),
        ("FileDescriptorStoreMax", "1"),
        ("FileDescriptorStorePreserve", "yes"),
        ("NotifyAccess", "all"),
        ("Restart", "no"),
        ("KillMode", "control-group"),
        ("StopWhenUnneeded", "no"),
        ("InvocationID", invocation),
    ] {
        ensure!(
            values.get(key).map(String::as_str) == Some(expected),
            "Native capacity holder has an unsafe or changed {key}"
        );
    }
    Ok(())
}

/// Re-observe the independent holder after its setup process has exited. A
/// journal's Ready phase alone never proves the capacity lock still exists.
pub(super) fn require_ready(run_id: &str, invocation: &str) -> Result<()> {
    let run = uuid::Uuid::parse_str(run_id)?;
    ensure!(
        !run.is_nil() && run.to_string() == run_id,
        "Invalid native holder run"
    );
    let unit = format!("gah-native-capacity-{run}.service");
    let values = state(&unit)?;
    validate_contract(&values, &unit, invocation)?;
    for (key, expected) in [
        ("ActiveState", "active"),
        ("SubState", "exited"),
        ("MainPID", "0"),
        ("NFileDescriptorStore", "1"),
    ] {
        ensure!(
            values.get(key).map(String::as_str) == Some(expected),
            "Native capacity holder is no longer ready"
        );
    }
    Ok(())
}

/// Kernel process identity, excluding zombies and reused PIDs.
pub(super) fn process_start_ticks(pid: u32) -> Result<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .context("Invalid native process identity")?
        .1
        .split_whitespace()
        .collect();
    ensure!(
        fields
            .first()
            .is_some_and(|state| !matches!(*state, "Z" | "X"))
            && fields.len() > 19,
        "Native job process is no longer executable"
    );
    let ticks = fields[19];
    ensure!(
        ticks.parse::<u64>().is_ok_and(|value| value > 0),
        "Invalid native process start time"
    );
    Ok(ticks.into())
}

/// Identity pinned to one launched process, independent of its service name.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct JobIdentity {
    pub(super) invocation: String,
    pub(super) pid: u32,
    pub(super) start_ticks: String,
}

/// Observe and recheck the actual running job before recording its identity.
pub(super) fn observe_job(run_id: &str) -> Result<JobIdentity> {
    let values = state(&format!("gah-native-job-{run_id}.service"))?;
    let invocation = values
        .get("InvocationID")
        .context("Native job identity unavailable")?
        .clone();
    let pid = values
        .get("MainPID")
        .context("Native job PID unavailable")?
        .parse()?;
    let start_ticks = process_start_ticks(pid)?;
    require_job(run_id, &invocation, pid, &start_ticks)?;
    Ok(JobIdentity {
        invocation,
        pid,
        start_ticks,
    })
}

/// Recheck a recorded job without treating service-name reuse as continuity.
pub(super) fn require_job(run_id: &str, invocation: &str, pid: u32, ticks: &str) -> Result<()> {
    let run = uuid::Uuid::parse_str(run_id)?;
    ensure!(
        !run.is_nil() && run.to_string() == run_id && pid > 0,
        "Invalid native job identity"
    );
    let unit = format!("gah-native-job-{run}.service");
    let values = state(&unit)?;
    let group = std::fs::read_to_string(format!("/proc/{pid}/cgroup"))?;
    let group = group
        .strip_prefix("0::")
        .and_then(|value| value.strip_suffix('\n'))
        .filter(|value| {
            value.starts_with('/') && !value.contains('\n') && value.ends_with(&format!("/{unit}"))
        })
        .context("Native job is outside its recorded service cgroup")?;
    for (key, expected) in [
        ("Id", unit.as_str()),
        ("LoadState", "loaded"),
        ("ActiveState", "active"),
        ("SubState", "running"),
        ("Transient", "yes"),
        ("Type", "exec"),
        ("ExitType", "cgroup"),
        ("RemainAfterExit", "yes"),
        ("Restart", "no"),
        ("KillMode", "control-group"),
        ("StopWhenUnneeded", "no"),
        ("InvocationID", invocation),
        ("ControlGroup", group),
    ] {
        ensure!(
            values.get(key).map(String::as_str) == Some(expected),
            "Native job has an unsafe or changed {key}"
        );
    }
    ensure!(
        values.get("MainPID") == Some(&pid.to_string()) && process_start_ticks(pid)? == ticks,
        "Native job process identity changed"
    );
    Ok(())
}

pub(super) fn require_own_job(run_id: &str, invocation: &str, pid: u32, ticks: &str) -> Result<()> {
    ensure!(
        pid == std::process::id() && std::env::var("INVOCATION_ID").as_deref() == Ok(invocation),
        "Native operation belongs to a different execution process"
    );
    require_job(run_id, invocation, pid, ticks)
}

/// Observe termination of the exact launched process and its entire cgroup.
/// Missing unit metadata or a lingering descendant cannot produce a receipt.
pub(super) fn terminal_job(
    run_id: &str,
    identity: &JobIdentity,
) -> Result<crate::ledger::remote_dispatch::ProcessTermination> {
    let unit = format!("gah-native-job-{run_id}.service");
    let values = state(&unit)?;
    let termination = terminal_state(&values, &unit, identity)?;
    ensure!(
        !matches!(process_start_ticks(identity.pid), Ok(ticks) if ticks == identity.start_ticks),
        "Native job's original process still exists"
    );
    let slice = state("app.slice")?;
    let parent = slice
        .get("ControlGroup")
        .filter(|group| {
            group.starts_with('/')
                && group.ends_with("/app.slice")
                && group
                    .split('/')
                    .any(|part| part == format!("user@{}.service", unsafe { libc::geteuid() }))
                && !group.split('/').any(|part| part == "..")
        })
        .context("Native job cgroup parent is unavailable")?;
    let group = format!("{parent}/{unit}");
    ensure!(
        values
            .get("ControlGroup")
            .is_some_and(|value| value.is_empty() || value == &group),
        "Native terminal job changed cgroup"
    );
    let directory = std::path::Path::new("/sys/fs/cgroup").join(group.trim_start_matches('/'));
    match std::fs::read_to_string(directory.join("cgroup.events")) {
        Ok(events) => ensure!(
            events.lines().any(|line| line == "populated 0"),
            "Native job descendants remain alive"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !directory.exists() => {}
        Err(_) => anyhow::bail!("Native job cgroup state is unavailable"),
    }
    ensure!(
        terminal_state(&state(&unit)?, &unit, identity)? == termination,
        "Native job termination changed during observation"
    );
    Ok(termination)
}

fn terminal_state(
    values: &BTreeMap<String, String>,
    unit: &str,
    identity: &JobIdentity,
) -> Result<crate::ledger::remote_dispatch::ProcessTermination> {
    use crate::ledger::remote_dispatch::{ProcessTermination, TerminationReason};
    for (key, expected) in [
        ("Id", unit),
        ("LoadState", "loaded"),
        ("Transient", "yes"),
        ("Type", "exec"),
        ("ExitType", "cgroup"),
        ("RemainAfterExit", "yes"),
        ("Restart", "no"),
        ("KillMode", "control-group"),
        ("StopWhenUnneeded", "no"),
        ("InvocationID", identity.invocation.as_str()),
        ("MainPID", "0"),
        ("Slice", "app.slice"),
    ] {
        ensure!(
            values.get(key).map(String::as_str) == Some(expected),
            "Native job termination has an unsafe or changed {key}"
        );
    }
    ensure!(
        matches!(
            (
                values.get("ActiveState").map(String::as_str),
                values.get("SubState").map(String::as_str)
            ),
            (Some("active"), Some("exited")) | (Some("failed"), Some("failed"))
        ),
        "Native job is not terminal"
    );
    ensure!(
        values.get("ExecMainPID") == Some(&identity.pid.to_string())
            && values
                .get("ExecMainExitTimestampMonotonic")
                .is_some_and(|value| value.parse::<u64>().is_ok_and(|value| value > 0)),
        "Native main process exit is unconfirmed"
    );
    let status: u32 = values
        .get("ExecMainStatus")
        .context("Native exit status unavailable")?
        .parse()?;
    let code: i32 = values
        .get("ExecMainCode")
        .context("Native exit code unavailable")?
        .parse()?;
    let (exit_code, signal) = match code {
        libc::CLD_EXITED => (Some(status), None),
        libc::CLD_KILLED | libc::CLD_DUMPED if status > 0 && status < 128 => (
            None,
            Some(match status as i32 {
                libc::SIGKILL => "SIGKILL".into(),
                libc::SIGTERM => "SIGTERM".into(),
                libc::SIGINT => "SIGINT".into(),
                libc::SIGABRT => "SIGABRT".into(),
                libc::SIGSEGV => "SIGSEGV".into(),
                other => format!("SIG{other}"),
            }),
        ),
        _ => anyhow::bail!("Native process termination is unsupported or unconfirmed"),
    };
    Ok(ProcessTermination {
        reason: TerminationReason::Exited,
        exit_code,
        signal,
    })
}

/// Reserve actual host capacity for a verified context and pass its locked
/// descriptor to this run's systemd holder. The caller must wait for the holder
/// to become active/exited with one stored FD before accepting this response.
/// The future executor must keep the holder until its separate job cgroup is
/// empty and its durable start state excludes a pending or concurrent launch.
pub fn reserve(context: &WorkerContext, work: &std::path::Path) -> Result<serde_json::Value> {
    let run = uuid::Uuid::parse_str(&context.binding.run_id)?;
    ensure!(
        !run.is_nil() && run.to_string() == context.binding.run_id,
        "Invalid native reservation run"
    );
    let unit = format!("gah-native-capacity-{run}.service");
    let invocation =
        std::env::var("INVOCATION_ID").context("Native admission requires a systemd holder")?;
    ensure!(
        invocation.len() == 32
            && invocation.bytes().all(|b| b.is_ascii_hexdigit())
            && invocation.bytes().any(|b| b != b'0'),
        "Invalid native holder invocation"
    );
    let groups = std::fs::read_to_string("/proc/self/cgroup")?;
    let own_cgroup = groups
        .strip_prefix("0::")
        .and_then(|value| value.strip_suffix('\n'))
        .filter(|value| {
            value.starts_with('/') && !value.contains('\n') && value.ends_with(&format!("/{unit}"))
        })
        .context("Native admission requires its own unified systemd cgroup")?;
    validate_holder(&state(&unit)?, &unit, &invocation, own_cgroup, false)?;
    let mut worker = super::worker::WorkerRun::reserve(work, &context.binding, &invocation)?;
    let mut lease = match crate::controller::acquire_native_capacity(
        &context.binding.mode,
        context.config.defaults.node_capacity,
    )? {
        crate::controller::NodeCapacityAdmission::Admit(lease) => lease,
        crate::controller::NodeCapacityAdmission::Defer(reason) => {
            anyhow::bail!("Native host admission deferred: {reason}")
        }
    };
    // Set retention before the first possible transfer, including failed or
    // timed-out notifications. Never unlink a possibly manager-owned lease.
    let file = lease.persistent_monitor_file()?;
    record_lease(context, work, &invocation, &file)?;
    let fd = file.as_raw_fd();
    let mut command = Command::new("/usr/bin/systemd-notify");
    command.args([
        "--fd=3",
        "--fdname=gah-native-capacity",
        "FDSTORE=1",
        "FDPOLL=0",
    ]);
    // SAFETY: only async-signal-safe descriptor operations run after fork. The
    // source File remains alive through run_bounded, and descriptor 3 is private
    // to the child. systemd-notify's default barrier confirms manager receipt.
    unsafe {
        command.pre_exec(move || {
            if (fd != 3 && libc::dup2(fd, 3) < 0) || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = crate::runner::process::run_bounded(command, Duration::from_secs(2))
        .context("Native reservation transfer was not confirmed; execution remains held")?;
    ensure!(
        output.status.success(),
        "Native reservation transfer was not confirmed; execution remains held"
    );
    validate_holder(&state(&unit)?, &unit, &invocation, own_cgroup, true)?;
    worker.ready()?;
    Ok(serde_json::json!({
        "binding": context.binding, "holder_unit": unit, "holder_invocation": invocation,
        "capacity_reserved": true, "execution_authorized": false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_state_requires_exact_exit_identity_and_preserves_failure_or_signal() {
        let unit = "gah-native-job-fixture.service";
        let identity = JobIdentity {
            invocation: "a".repeat(32),
            pid: 123,
            start_ticks: "1".into(),
        };
        let mut values: BTreeMap<String, String> = [
            ("Id", unit),
            ("LoadState", "loaded"),
            ("Transient", "yes"),
            ("Type", "exec"),
            ("ExitType", "cgroup"),
            ("RemainAfterExit", "yes"),
            ("Restart", "no"),
            ("KillMode", "control-group"),
            ("StopWhenUnneeded", "no"),
            ("InvocationID", &identity.invocation),
            ("MainPID", "0"),
            ("Slice", "app.slice"),
            ("ActiveState", "failed"),
            ("SubState", "failed"),
            ("ExecMainPID", "123"),
            ("ExecMainExitTimestampMonotonic", "100"),
            ("ExecMainCode", "1"),
            ("ExecMainStatus", "7"),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
        assert_eq!(
            terminal_state(&values, unit, &identity).unwrap().exit_code,
            Some(7)
        );
        for (key, wrong) in [
            ("InvocationID", "replaced"),
            ("MainPID", "123"),
            ("ExecMainPID", "999"),
            ("ExecMainExitTimestampMonotonic", "0"),
            ("SubState", "running"),
            ("ExitType", "main"),
        ] {
            let mut changed = values.clone();
            changed.insert(key.into(), wrong.into());
            assert!(
                terminal_state(&changed, unit, &identity).is_err(),
                "accepted changed {key}"
            );
        }
        values.insert("ExecMainCode".into(), libc::CLD_KILLED.to_string());
        values.insert("ExecMainStatus".into(), libc::SIGKILL.to_string());
        let killed = terminal_state(&values, unit, &identity).unwrap();
        assert_eq!(killed.signal.as_deref(), Some("SIGKILL"));
        assert!(killed.exit_code.is_none());
    }

    #[test]
    fn holder_requires_exact_process_invocation_and_retention_contract() {
        let unit = "gah-native-capacity-aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa.service";
        let invocation = "a".repeat(32);
        let group = format!("/user.slice/{unit}");
        let mut values: BTreeMap<String, String> = [
            ("Id", unit),
            ("LoadState", "loaded"),
            ("ActiveState", "activating"),
            ("SubState", "start"),
            ("Transient", "yes"),
            ("Type", "oneshot"),
            ("RemainAfterExit", "yes"),
            ("FileDescriptorStoreMax", "1"),
            ("NFileDescriptorStore", "0"),
            ("FileDescriptorStorePreserve", "yes"),
            ("NotifyAccess", "all"),
            ("Restart", "no"),
            ("KillMode", "control-group"),
            ("StopWhenUnneeded", "no"),
            ("InvocationID", &invocation),
            ("ControlGroup", &group),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        values.insert("MainPID".into(), std::process::id().to_string());
        validate_holder(&values, unit, &invocation, &group, false).unwrap();
        for key in values.keys().cloned().collect::<Vec<_>>() {
            let mut invalid = values.clone();
            invalid.insert(key, "unknown".into());
            assert!(validate_holder(&invalid, unit, &invocation, &group, false).is_err());
        }
        assert!(validate_holder(&values, unit, &invocation, &group, true).is_err());
        values.insert("NFileDescriptorStore".into(), "1".into());
        validate_holder(&values, unit, &invocation, &group, true).unwrap();
    }
}
