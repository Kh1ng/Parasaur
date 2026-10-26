//! Persist native Antigravity readings under their independent model pools.
use super::{append, append_scoped, QuotaObservationRecord};
use anyhow::Result;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// The store instance that records when this probe last ran for an account.
/// It is not a routing scope, so a failed or empty check filed under it
/// throttles the next probe without invalidating any pool's readings,
/// including ones imported from another source.
pub(super) fn probe_instance(account: &str) -> String {
    format!("{account}:usage-probe")
}

/// The accounts the scheduled refresh samples for this profile, with the
/// HOME each one runs under. `agy-second` is sampled only with its own home.
pub(super) fn accounts(profile: &crate::config::Profile) -> Vec<(&'static str, Option<PathBuf>)> {
    let second = profile
        .agy_second_home
        .as_deref()
        .filter(|home| !home.trim().is_empty())
        .map(|home| ("agy-second", Some(PathBuf::from(home))));
    std::iter::once(("agy", None)).chain(second).collect()
}

/// Background refresh threads for each configured Antigravity account.
pub(super) fn refresh_handles(
    profile: &crate::config::Profile,
    store_path: &Path,
    now: OffsetDateTime,
) -> Vec<std::thread::JoinHandle<()>> {
    let mut handles = Vec::new();
    for (account, home) in accounts(profile) {
        let path = store_path.to_path_buf();
        let command = profile
            .configured_backend_path(account)
            .unwrap_or("agy")
            .to_string();
        let probe = probe_instance(account);
        let refresh = move || refresh_and_store(&command, account, home.as_deref(), &path);
        handles.extend(super::maybe_refresh_backend_instance(
            store_path,
            account,
            Some(&probe),
            now,
            refresh,
        ));
    }
    handles
}

/// Executable and HOME for an explicit `gah quota refresh --backend <account>`,
/// taken from the first profile that configures them.
pub fn launch(
    config: &crate::config::GahConfig,
    account: &str,
) -> Result<(String, Option<PathBuf>)> {
    let executable = config
        .profiles
        .values()
        .find_map(|profile| profile.configured_backend_path(account))
        .unwrap_or("agy")
        .to_string();
    let home = config
        .profiles
        .values()
        .flat_map(accounts)
        .find(|(name, _)| *name == account)
        .map(|(_, home)| home);
    match home {
        Some(home) => Ok((executable, home)),
        None => {
            anyhow::bail!("no profile sets agy_second_home, so {account} has no account to check")
        }
    }
}

pub fn refresh_and_store(
    executable: &str,
    account: &str,
    home: Option<&Path>,
    state_path: &Path,
) -> Result<Option<QuotaObservationRecord>> {
    let records = crate::usage::agy::refresh(executable, account, home)?;
    let now = crate::clock::now_utc();
    let records = with_retired_scopes(state_path, account, records, now)?;
    let first = append_scoped(state_path, records, None)?;
    append(state_path, &probe_marker(account, now))?;
    Ok(first)
}

/// A scope (pool, window) this account reported before but not in this
/// successful response gets a failed check, so its last balance stops driving
/// routing instead of surviving until it ages out. Mirrors `AgyQuotaScopes`
/// in apps/server: only a successful inventory retires scopes; a failed probe
/// never reaches here and keeps every reading.
fn with_retired_scopes(
    state_path: &Path,
    account: &str,
    mut records: Vec<QuotaObservationRecord>,
    now: OffsetDateTime,
) -> Result<Vec<QuotaObservationRecord>> {
    let prefix = format!("{account}:");
    let timestamp = now.format(&Rfc3339)?;
    let scope = |record: &QuotaObservationRecord| {
        (
            record.backend.clone(),
            record.backend_instance.clone(),
            record.quota_pool.clone(),
            record.quota_window.clone(),
        )
    };
    let mut seen: std::collections::HashSet<_> = records.iter().map(scope).collect();
    for previous in super::load(state_path)? {
        let is_scope = previous.usage_source.as_deref() == Some(crate::usage::agy::USAGE_SOURCE)
            && previous
                .quota_pool
                .as_deref()
                .is_some_and(|pool| pool.starts_with(&prefix))
            && previous.quota_window.is_some();
        if !is_scope || !seen.insert(scope(&previous)) {
            continue;
        }
        let (pool, window) = (
            previous.quota_pool.clone().unwrap_or_default(),
            previous.quota_window.clone().unwrap_or_default(),
        );
        records.push(QuotaObservationRecord {
            backend: previous.backend,
            backend_instance: previous.backend_instance,
            credential_id: None,
            model: None,
            quota_pool: previous.quota_pool,
            quota_window: previous.quota_window,
            quota_remaining_percent: None,
            quota_reset_at: None,
            observed_at: None,
            checked_at: Some(timestamp.clone()),
            check_error: Some(format!(
                "Antigravity usage no longer reports the {window} window for {pool}"
            )),
            usage_source: Some(crate::usage::agy::USAGE_SOURCE.into()),
            account_usage: None,
        });
    }
    Ok(records)
}

fn probe_marker(account: &str, now: OffsetDateTime) -> QuotaObservationRecord {
    QuotaObservationRecord {
        backend: account.into(),
        backend_instance: Some(probe_instance(account)),
        credential_id: None,
        model: None,
        quota_pool: None,
        quota_window: None,
        quota_remaining_percent: None,
        quota_reset_at: None,
        observed_at: None,
        checked_at: now.format(&Rfc3339).ok(),
        check_error: None,
        usage_source: Some(crate::usage::agy::USAGE_SOURCE.into()),
        account_usage: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_identity::ExecutionIdentity;

    #[test]
    fn agy_probe_marker_uses_the_injected_clock() {
        let now = OffsetDateTime::parse("2030-01-01T00:00:00Z", &Rfc3339).unwrap();
        let _clock = crate::clock::set_for_test(now);
        let marker = probe_marker("agy", crate::clock::now_utc());
        assert_eq!(marker.checked_at.as_deref(), Some("2030-01-01T00:00:00Z"));
    }

    #[test]
    fn agy_probe_is_not_overdue_until_the_refresh_interval_passes() {
        let now = OffsetDateTime::parse("2030-01-01T00:00:00Z", &Rfc3339).unwrap();
        let _clock = crate::clock::set_for_test(now);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.jsonl");
        append(
            &path,
            &probe_marker("agy-clock-test", crate::clock::now_utc()),
        )
        .unwrap();
        assert!(super::super::maybe_refresh_backend_instance(
            &path,
            "agy-clock-test",
            Some(&probe_instance("agy-clock-test")),
            now + time::Duration::minutes(14),
            || panic!("probe must not be overdue at the interval boundary"),
        )
        .is_none());
        super::super::maybe_refresh_backend_instance(
            &path,
            "agy-clock-test",
            Some(&probe_instance("agy-clock-test")),
            now + time::Duration::minutes(14) + time::Duration::seconds(1),
            || Ok(None),
        )
        .expect("probe becomes due after the interval")
        .join()
        .unwrap();
    }

    fn reading(backend: &str, source: &str, now: OffsetDateTime) -> QuotaObservationRecord {
        serde_json::from_value(serde_json::json!({
            "backend": backend,
            "backend_instance": format!("{backend}:google-native"),
            "quota_pool": format!("{backend}:google-native"),
            "quota_window": "weekly",
            "quota_remaining_percent": 80.0,
            "usage_source": source,
            "checked_at": now.format(&Rfc3339).unwrap(),
            "observed_at": now.format(&Rfc3339).unwrap()
        }))
        .unwrap()
    }

    #[test]
    fn only_this_probes_own_check_throttles_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.jsonl");
        let now = OffsetDateTime::now_utc();
        // Account names no other test probes: in-flight checks are tracked
        // process-wide, so a shared name would race a sibling test.
        for account in ["agy-throttle-one", "agy-throttle-two"] {
            // A fresh reading imported from another source does not count as
            // this probe having run.
            append(&path, &reading(account, "cli_router", now)).unwrap();
            let due = super::super::maybe_refresh_backend_instance(
                &path,
                account,
                Some(&probe_instance(account)),
                now,
                || Ok(None),
            );
            due.expect("probe is due").join().unwrap();
            assert!(super::super::maybe_refresh_backend_instance(
                &path,
                account,
                Some(&probe_instance(account)),
                now,
                || panic!("not due")
            )
            .is_none());
        }
    }

    #[test]
    fn a_failed_probe_keeps_readings_from_other_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quota.jsonl");
        let now = OffsetDateTime::now_utc();
        append(&path, &reading("agy", "cli_router", now)).unwrap();
        super::super::maybe_refresh_backend_instance(
            &path,
            "agy",
            Some(&probe_instance("agy")),
            now + time::Duration::minutes(1),
            || anyhow::bail!("agy is not on PATH"),
        )
        .expect("probe is due")
        .join()
        .unwrap();

        let records = super::super::load(&path).unwrap();
        assert!(records.iter().any(|record| record.check_error.is_some()
            && record.backend_instance == Some(probe_instance("agy"))));
        let identity = ExecutionIdentity::legacy_candidate(
            "agy",
            Some("Gemini 3.5 Flash"),
            Some("agy:google-native"),
        );
        let windows = super::super::latest_windows_for_identity(&records, &identity);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].quota_remaining_percent, Some(80.0));
    }

    #[test]
    fn an_empty_second_home_is_not_an_account() {
        let mut profile = crate::config::tests::test_profile_for_notifications();
        profile.agy_second_home = Some("  ".into());
        assert_eq!(accounts(&profile), vec![("agy", None)]);
        profile.agy_second_home = Some("/accounts/second".into());
        assert_eq!(
            accounts(&profile)[1],
            ("agy-second", Some(PathBuf::from("/accounts/second")))
        );
    }

    #[test]
    fn a_window_missing_from_a_successful_response_retires_its_last_balance() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("quota.jsonl");
        let now = OffsetDateTime::parse("2030-01-01T00:00:00Z", &Rfc3339).unwrap();
        append(&path, &reading("agy", crate::usage::agy::USAGE_SOURCE, now)).unwrap();
        append(&path, &reading("agy", "cli_router", now)).unwrap();
        let mut fresh = reading("agy", crate::usage::agy::USAGE_SOURCE, now);
        fresh.backend_instance = Some("agy:external".into());
        fresh.quota_pool = Some("agy:external".into());
        let later = now + time::Duration::hours(1);
        let records = super::with_retired_scopes(&path, "agy", vec![fresh], later).unwrap();
        assert_eq!(records.len(), 2);
        let retired = &records[1];
        assert_eq!(retired.quota_pool.as_deref(), Some("agy:google-native"));
        assert_eq!(retired.quota_window.as_deref(), Some("weekly"));
        assert_eq!(retired.quota_remaining_percent, None);
        assert_eq!(retired.observed_at, None);
        assert_eq!(retired.checked_at.as_deref(), Some("2030-01-01T01:00:00Z"));
        assert!(retired
            .check_error
            .as_deref()
            .unwrap()
            .contains("no longer reports"));
        // Another account's scopes and other sources are untouched.
        let other = super::with_retired_scopes(&path, "agy-second", vec![], later).unwrap();
        assert!(other.is_empty());
        // Reporting the window again produces no retirement record.
        let again = super::with_retired_scopes(
            &path,
            "agy",
            vec![reading("agy", crate::usage::agy::USAGE_SOURCE, later)],
            later,
        )
        .unwrap();
        assert_eq!(again.len(), 1);
    }
}
