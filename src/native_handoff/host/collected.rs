//! Once systemd collects a transient unit, only its user manager's journal can
//! attest the original launcher's exit. This evidence is for unused starts only.
use super::*;
use crate::ledger::remote_dispatch::DispatchBinding;
use std::path::Path;

pub(super) fn unit_collected(values: &BTreeMap<String, String>, unit: &str) -> bool {
    [
        ("Id", unit),
        ("LoadState", "not-found"),
        ("ActiveState", "inactive"),
        ("SubState", "dead"),
        ("InvocationID", ""),
        ("MainPID", "0"),
        ("NFileDescriptorStore", "0"),
    ]
    .into_iter()
    .all(|(key, expected)| values.get(key).map(String::as_str) == Some(expected))
}

pub(in crate::native_handoff) fn collected_unused_exit(
    binding: &DispatchBinding,
    holder: &str,
    job: &JobIdentity,
    work: &Path,
) -> Result<()> {
    let unit = format!("gah-native-job-{}.service", binding.run_id);
    let holder_unit = format!("gah-native-capacity-{}.service", binding.run_id);
    ensure!(
        unit_collected(&state(&unit)?, &unit)
            && unit_collected(&state(&holder_unit)?, &holder_unit),
        "Native job or capacity unit collection unconfirmed"
    );
    match process_start_ticks(job.pid) {
        Ok(ticks) => ensure!(
            ticks != job.start_ticks,
            "Original native process is still present"
        ),
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) => {}
        Err(e) => return Err(e),
    }
    // Hold the original lease lock, if its entry still exists, while checking
    // the exit record. This also checks that the job's cgroup is empty.
    let _verifier = release_unused_capacity(binding, holder, Some(job), work, false)?;
    let uid = unsafe { libc::geteuid() };
    let machine = std::fs::read_to_string("/etc/machine-id")?;
    let mut command = Command::new("/usr/bin/journalctl");
    command.args([
        "--quiet",
        "--no-pager",
        "--output=cat",
        "--output-fields=EXIT_STATUS",
        "--lines=1",
        "MESSAGE_ID=98e322203f7a4ed290d09fe03c09fe15",
        "COMMAND=ExecStart",
        "EXIT_CODE=exited",
        "_EXE=/usr/lib/systemd/systemd",
        "_SYSTEMD_USER_UNIT=init.scope",
        "_TRANSPORT=journal",
        &format!("_MACHINE_ID={}", machine.trim()),
        &format!("_UID={uid}"),
        &format!("_SYSTEMD_UNIT=user@{uid}.service"),
        &format!("USER_UNIT={unit}"),
        &format!("USER_INVOCATION_ID={}", job.invocation),
    ]);
    let output = crate::runner::process::run_bounded(command, Duration::from_secs(3))
        .context("Native launcher exit journal unavailable")?;
    ensure!(
        output.status.success()
            && std::str::from_utf8(&output.stdout)?
                .trim()
                .parse::<u8>()
                .is_ok_and(|code| code > 0),
        "Original native launcher has no confirmed failed exit"
    );
    Ok(())
}
