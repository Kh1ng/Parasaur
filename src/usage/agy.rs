//! Account quota reported by the native Antigravity `/usage` slash command.
use crate::quota_store::QuotaObservationRecord;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub const USAGE_SOURCE: &str = "agy_cli_usage";

/// Execute a metadata command within a shared probe deadline.
pub(crate) fn run_until(
    executable: &str,
    home: Option<&Path>,
    args: &[&str],
    deadline: Instant,
) -> Result<Vec<u8>> {
    let mut command = Command::new(executable);
    command.args(args);
    if let Some(home) = home {
        command.env("HOME", home);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() == 1 {
                    libc::kill(libc::getpid(), libc::SIGKILL);
                }
                Ok(())
            });
        }
    }
    let timeout = deadline.saturating_duration_since(Instant::now());
    let output = crate::runner::process::run_bounded(command, timeout)
        .context("Antigravity metadata command timed out or could not be started")?;
    if !output.status.success() {
        bail!("Antigravity metadata command failed ({})", output.status);
    }
    // AGY sends --help to stderr in 1.3.6.
    Ok(if args == ["--help"] {
        [output.stdout, output.stderr].concat()
    } else {
        output.stdout
    })
}

/// Probe only builds that explicitly support native slash commands in print mode.
pub(crate) fn read_usage(executable: &str, home: Option<&Path>) -> Result<Vec<u8>> {
    read_usage_until(executable, home, None)
}

/// Apply the native-command gate within a caller's deadline, or the quota
/// reader's separate 10-second help and 30-second usage timeouts.
pub(crate) fn read_usage_until(
    executable: &str,
    home: Option<&Path>,
    deadline: Option<Instant>,
) -> Result<Vec<u8>> {
    let help = run_until(
        executable,
        home,
        &["--help"],
        deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(10)),
    )?;
    if !String::from_utf8_lossy(&help).contains("--disable-slash-commands") {
        bail!("this Antigravity build does not run slash commands in print mode; usage is not sampled");
    }
    run_until(
        executable,
        home,
        &["--print", "/usage", "--output-format", "json"],
        deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(30)),
    )
}

/// Read all provider-reported windows for one account. No model call is made.
pub fn refresh(
    executable: &str,
    account: &str,
    home: Option<&Path>,
) -> Result<Vec<QuotaObservationRecord>> {
    parse(
        &read_usage(executable, home)?,
        account,
        crate::clock::now_utc(),
    )
}

/// Logical backends that draw on one account's quota. Readings are stored
/// once per name, because a route only sees readings for its own backend.
fn logical_backends(account: &str) -> Vec<&str> {
    let mut backends = vec![account];
    backends.extend(
        ["agy-main"]
            .into_iter()
            .filter(|alias| crate::availability::agy_account(alias) == Some(account)),
    );
    backends
}

fn parse(output: &[u8], account: &str, now: OffsetDateTime) -> Result<Vec<QuotaObservationRecord>> {
    let groups = crate::availability::agy::groups(output)?;
    let timestamp = now.format(&Rfc3339)?;
    let mut records = Vec::new();
    for group in &groups {
        let pool = group.pool();
        for bucket in &group.buckets {
            let window = &bucket.window;
            // The CLI omits zero-valued fields for an exhausted reported bucket.
            let fraction = bucket.remaining_fraction.unwrap_or(0.0);
            let fraction =
                (fraction.is_finite() && (0.0..=1.0).contains(&fraction)).then_some(fraction);
            for backend in logical_backends(account) {
                records.push(QuotaObservationRecord {
                    backend: backend.into(),
                    backend_instance: Some(format!("{backend}:{pool}")),
                    credential_id: None,
                    model: None,
                    quota_pool: Some(format!("{account}:{pool}")),
                    quota_window: Some(window.clone()),
                    quota_remaining_percent: fraction.map(|fraction| fraction * 100.0),
                    quota_reset_at: bucket
                        .reset_time
                        .as_deref()
                        .filter(|value| OffsetDateTime::parse(value, &Rfc3339).is_ok())
                        .map(str::to_owned),
                    observed_at: fraction.map(|_| timestamp.clone()),
                    checked_at: Some(timestamp.clone()),
                    check_error: fraction.is_none().then(|| {
                        format!(
                            "Antigravity usage reported no readable {window} balance for {pool}"
                        )
                    }),
                    usage_source: Some(USAGE_SOURCE.into()),
                    account_usage: None,
                });
            }
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    const USAGE: &[u8] = include_bytes!("../../tests/fixtures/agy/usage.json");

    #[test]
    fn parses_distinct_pools_and_windows_from_recorded_native_usage() {
        let records = parse(USAGE, "agy-second", OffsetDateTime::UNIX_EPOCH).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(
            records[0].backend_instance.as_deref(),
            Some("agy-second:google-native")
        );
        assert_eq!(records[0].quota_window.as_deref(), Some("weekly"));
        assert_eq!(records[0].quota_remaining_percent, Some(0.0));
        assert_eq!(
            records[1].quota_pool.as_deref(),
            Some("agy-second:external")
        );
        assert_eq!(records[1].quota_remaining_percent, Some(100.0));
        assert!(records.iter().all(|record| record.check_error.is_none()));
    }

    #[cfg(unix)]
    #[test]
    fn agy_refresh_observations_use_the_injected_clock() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("agy");
        std::fs::write(
            &executable,
            "#!/bin/sh\ncase \"$*\" in\n  --help) echo --disable-slash-commands ;;\n  *) echo '{\"status\":\"SUCCESS\",\"command\":{\"name\":\"usage\",\"data\":{\"groups\":[{\"buckets\":[{\"id\":\"gemini-weekly\",\"window\":\"weekly\",\"remaining_fraction\":0.8},{\"id\":\"gemini-5h\",\"window\":\"5h\",\"remaining_fraction\":0.7}]},{\"buckets\":[{\"id\":\"3p-weekly\",\"window\":\"weekly\",\"remaining_fraction\":0.6},{\"id\":\"3p-5h\",\"window\":\"5h\",\"remaining_fraction\":0.5}]}]}}}' ;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let now = OffsetDateTime::parse("2030-01-01T00:00:00Z", &Rfc3339).unwrap();
        let _clock = crate::clock::set_for_test(now);
        let records = refresh(executable.to_str().unwrap(), "agy", None).unwrap();
        assert_eq!(records.len(), 8);
        assert!(records
            .iter()
            .all(|record| record.checked_at.as_deref() == Some("2030-01-01T00:00:00Z")));
    }

    #[test]
    fn default_account_readings_match_routing_and_aliases() {
        let _fixture = crate::availability::agy::fixture();
        let records = parse(USAGE, "agy", OffsetDateTime::UNIX_EPOCH).unwrap();
        for backend in ["agy", "agy-main"] {
            let identity = crate::execution_identity::ExecutionIdentity::legacy_candidate(
                backend,
                Some("Gemini 3.8 Flash (High)"),
                crate::availability::derive_quota_pool(
                    backend,
                    Some("Gemini 3.8 Flash (High)"),
                    Some(&_fixture.catalog),
                ),
            );
            let windows = crate::quota_store::latest_windows_for_identity(&records, &identity);
            assert_eq!(windows.len(), 1);
            assert_eq!(windows[0].quota_remaining_percent, Some(0.0));
        }
        let second = crate::execution_identity::ExecutionIdentity::legacy_candidate(
            "agy-second",
            Some("Gemini 3.8 Flash (High)"),
            crate::availability::derive_quota_pool(
                "agy-second",
                Some("Gemini 3.8 Flash (High)"),
                Some(&_fixture.catalog),
            ),
        );
        assert!(crate::quota_store::latest_windows_for_identity(&records, &second).is_empty());
    }

    #[test]
    fn distinct_agy_provider_groups_route_to_their_own_weekly_balance() {
        let output = br#"{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[
            {"description":"Models within this group: Gemini Flash","buckets":[{"id":"gemini-flash-weekly","window":"weekly","remaining_fraction":0}]},
            {"description":"Models within this group: Gemini Pro","buckets":[{"id":"gemini-pro-weekly","window":"weekly","remaining_fraction":0.8}]},
            {"description":"Models within this group: Claude Sonnet","buckets":[{"id":"3p-sonnet-weekly","window":"weekly","remaining_fraction":0.3}]},
            {"description":"Models within this group: Claude Opus","buckets":[{"id":"3p-opus-weekly","window":"weekly","remaining_fraction":0.6}]}
        ]}}}"#;
        let catalog = crate::availability::AgyCatalog::parse(
            "flash\tGemini Flash\npro\tGemini Pro\nsonnet\tClaude Sonnet\nopus\tClaude Opus\n",
            output,
        )
        .unwrap();
        let records = parse(output, "agy", OffsetDateTime::UNIX_EPOCH).unwrap();
        for (model, pool, balance) in [
            ("flash", "gemini-flash-weekly", 0.0),
            ("pro", "gemini-pro-weekly", 80.0),
            ("sonnet", "3p-sonnet-weekly", 30.0),
            ("opus", "3p-opus-weekly", 60.0),
        ] {
            let routed_pool = catalog.pool("agy", model);
            assert_eq!(routed_pool, format!("agy:{pool}"));
            let identity = crate::execution_identity::ExecutionIdentity::legacy_candidate(
                "agy",
                Some(model),
                Some(routed_pool),
            );
            let windows = crate::quota_store::latest_windows_for_identity(&records, &identity);
            assert_eq!(windows.len(), 1);
            assert_eq!(windows[0].quota_remaining_percent, Some(balance));
        }
    }

    #[test]
    fn dynamic_windows_share_the_reported_group_without_a_bucket_table() {
        let output = br#"{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[{"buckets":[{"id":"nova-weekly","window":"weekly","reset_time":"2026-10-31T14:55:54Z"},{"id":"nova-short","window":"short","remaining_fraction":1.2}]}]}}}"#;
        let records = parse(output, "agy-second", OffsetDateTime::UNIX_EPOCH).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].quota_remaining_percent, Some(0.0));
        assert!(records[0].check_error.is_none());
        assert_eq!(records[0].quota_pool, records[1].quota_pool);
        assert_eq!(records[1].quota_window.as_deref(), Some("short"));
        assert!(records[1].quota_remaining_percent.is_none());
        assert!(records[1].observed_at.is_none());
        assert!(records[1].check_error.is_some());
    }

    #[test]
    fn rejects_responses_that_are_not_a_native_usage_result() {
        let now = OffsetDateTime::UNIX_EPOCH;
        for output in [
            r#"{"status":"ERROR","command":{"name":"usage","data":{"groups":[]}}}"#,
            r#"{"status":"SUCCESS","num_turns":1,"command":{"name":"usage","data":{"groups":[]}}}"#,
            r#"{"status":"SUCCESS","response":"Here is how usage works"}"#,
            r#"{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[{"buckets":[{"id":"missing-window","remaining_fraction":1}]}]}}}"#,
        ] {
            assert!(parse(output.as_bytes(), "agy", now).is_err(), "{output}");
        }
    }

    #[cfg(unix)]
    fn fake_agy(dir: &Path, help: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("fake-agy");
        let args = dir.join("args");
        std::fs::write(&script, format!(
            "#!/bin/sh\nif [ \"$1\" = --help ]; then echo '{help}' >&2; exit 0; fi\nprintf '%s|%s|%s|%s|%s' \"$1\" \"$2\" \"$3\" \"$4\" \"$HOME\" > '{}'\nprintf '%s' '{{\"status\":\"SUCCESS\",\"command\":{{\"name\":\"usage\",\"data\":{{\"groups\":[{{\"buckets\":[{{\"id\":\"gemini-weekly\",\"window\":\"weekly\",\"remaining_fraction\":0.4}}]}}]}}}}}}'\n",
            args.display()
        )).unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&script, permissions).unwrap();
        (script, args)
    }

    #[cfg(unix)]
    #[test]
    fn refresh_invokes_slash_command_and_keeps_selected_home() {
        let _exec = crate::test_support::ExecGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let (script, args) = fake_agy(dir.path(), "  --disable-slash-commands  Disable them");
        let selected_home = dir.path().join("selected-home");
        let records =
            refresh(script.to_str().unwrap(), "agy-second", Some(&selected_home)).unwrap();
        assert_eq!(records[0].quota_remaining_percent, Some(40.0));
        assert_eq!(
            std::fs::read_to_string(args).unwrap(),
            format!(
                "--print|/usage|--output-format|json|{}",
                selected_home.display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn refresh_never_sends_usage_to_a_build_without_print_mode_slash_commands() {
        let _exec = crate::test_support::ExecGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let (script, args) = fake_agy(dir.path(), "  --print  Run a single prompt");
        let error = refresh(script.to_str().unwrap(), "agy", None).unwrap_err();
        assert!(error.to_string().contains("slash commands"), "{error}");
        assert!(!args.exists(), "the usage prompt must not have been sent");
    }
}
