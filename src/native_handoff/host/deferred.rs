//! A failed admission unit keeps its transient name until systemd clears it.
//! Reset only the exact failed holder before any descriptor transfer or start.
use super::*;

pub(in crate::native_handoff) fn deferred_holder(
    run: &str,
    holder: &str,
    rearm: bool,
) -> Result<()> {
    release::job_empty(run, None)?;
    let unit = format!("gah-native-capacity-{run}.service");
    let values = state(&unit)?;
    if collected::unit_collected(&values, &unit) {
        return Ok(());
    }
    validate_contract(&values, &unit, holder)?;
    for (key, expected) in [
        ("ActiveState", "failed"),
        ("SubState", "failed"),
        ("MainPID", "0"),
        ("NFileDescriptorStore", "0"),
        ("ExecMainCode", "1"),
        ("ExecMainStatus", "1"),
    ] {
        ensure!(
            values.get(key).map(String::as_str) == Some(expected),
            "Native admission is still active or its failure is unconfirmed"
        );
    }
    if rearm {
        let mut command = Command::new("/usr/bin/systemctl");
        command.args(["--user", "reset-failed", &unit]);
        let output = crate::runner::process::run_bounded(command, Duration::from_secs(2))
            .context("Native deferred admission reset is unconfirmed")?;
        ensure!(
            output.status.success() && collected::unit_collected(&state(&unit)?, &unit),
            "Native deferred admission is not yet collected"
        );
    }
    Ok(())
}
