//! Host admission for native jobs. A separate active/exited systemd service
//! retains the ordinary capacity lock without a resident broker process.
//! Reserving capacity never authorizes backend execution or publication.
use super::WorkerContext;
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap, os::fd::AsRawFd, os::unix::process::CommandExt, process::Command,
    time::Duration,
};

fn state(unit: &str) -> Result<BTreeMap<String, String>> {
    let mut command = Command::new("/usr/bin/systemctl");
    command.args(["--user", "show", unit, "--property=Id,LoadState,ActiveState,SubState,MainPID,ControlGroup,InvocationID,Transient,Type,RemainAfterExit,FileDescriptorStoreMax,NFileDescriptorStore,FileDescriptorStorePreserve,NotifyAccess,Restart,KillMode,StopWhenUnneeded"]);
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
