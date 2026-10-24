//! #1539: retention and rotation for the durable account quota store.
//!
//! These pin the acceptance criteria: compaction keeps the latest N
//! observations per identity and window, drops old ones, preserves the
//! reading every selector returns (run before and after), never drops a
//! credential lifecycle marker, enforces the size backstop, and rotates
//! safely against concurrent appenders.

use super::*;
use crate::execution_identity::ExecutionIdentity;
use crate::quota_store::{
    append, latest_for_identity, latest_windows_for_backend,
    latest_windows_for_identity_and_credential, load, QuotaObservationRecord,
};
use std::path::PathBuf;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

fn tmp_store() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota_observations.jsonl");
    (dir, path)
}

fn ts(minutes: i64) -> String {
    let base = OffsetDateTime::parse("2026-10-20T00:00:00Z", &Rfc3339).unwrap();
    let time = base + time::Duration::minutes(minutes);
    time.format(&Rfc3339).unwrap()
}

fn observation(
    backend: &str,
    instance: Option<&str>,
    window: Option<&str>,
    checked_at: &str,
    remaining: f64,
) -> QuotaObservationRecord {
    QuotaObservationRecord {
        backend: backend.to_string(),
        credential_id: None,
        backend_instance: instance.map(str::to_string),
        model: None,
        quota_pool: None,
        quota_window: window.map(str::to_string),
        quota_remaining_percent: Some(remaining),
        quota_reset_at: None,
        observed_at: Some(checked_at.to_string()),
        checked_at: Some(checked_at.to_string()),
        check_error: None,
        usage_source: Some("test_refresh".to_string()),
        account_usage: None,
    }
}

/// A credential-scoped record; `usage_source` selects lifecycle markers.
fn credential_observation(
    credential_id: &str,
    instance: &str,
    window: Option<&str>,
    checked_at: &str,
    remaining: Option<f64>,
    usage_source: Option<&str>,
) -> QuotaObservationRecord {
    QuotaObservationRecord {
        backend: "mistral-dashboard".to_string(),
        credential_id: Some(credential_id.to_string()),
        backend_instance: Some(instance.to_string()),
        model: None,
        quota_pool: Some(instance.to_string()),
        quota_window: window.map(str::to_string),
        quota_remaining_percent: remaining,
        quota_reset_at: None,
        observed_at: Some(checked_at.to_string()),
        checked_at: Some(checked_at.to_string()),
        check_error: None,
        usage_source: usage_source.map(str::to_string),
        account_usage: None,
    }
}

fn reading_summary(records: &[&QuotaObservationRecord]) -> Vec<(Option<String>, Option<f64>)> {
    records
        .iter()
        .map(|record| (record.checked_at.clone(), record.quota_remaining_percent))
        .collect()
}

/// #1539: old observations are dropped, the latest N per identity and
/// window are kept, and the surviving file order is unchanged.
#[test]
fn compact_drops_old_observations_and_keeps_the_latest_per_window() {
    let (_dir, path) = tmp_store();
    for minute in 0..6 {
        append(
            &path,
            &observation(
                "codex",
                None,
                Some("five-hour"),
                &ts(minute),
                50.0 + minute as f64,
            ),
        )
        .unwrap();
    }

    let dropped = compact(
        &path,
        RetentionPolicy {
            max_per_window: 2,
            max_bytes: usize::MAX,
        },
        None,
    )
    .unwrap();

    assert_eq!(dropped, 4, "only the newest two observations survive");
    let records = load(&path).unwrap();
    assert_eq!(
        reading_summary(&records.iter().collect::<Vec<_>>()),
        vec![(Some(ts(4)), Some(54.0)), (Some(ts(5)), Some(55.0))],
        "the newest readings survive in file order"
    );
}

/// #1539 acceptance: run the selectors before and after compaction and
/// require identical answers, across several series (windows and instances).
#[test]
fn compact_preserves_the_reading_every_selector_returns() {
    let (_dir, path) = tmp_store();
    for (window, base) in [("five-hour", 0i64), ("weekly", 10i64)] {
        for step in 0..3 {
            append(
                &path,
                &observation(
                    "codex",
                    Some("codex"),
                    Some(window),
                    &ts(base + step),
                    50.0 + (base + step) as f64,
                ),
            )
            .unwrap();
        }
    }
    for step in 0..3 {
        append(
            &path,
            &observation(
                "codex",
                Some("codex-alt"),
                Some("five-hour"),
                &ts(20 + step),
                70.0,
            ),
        )
        .unwrap();
    }

    let before = load(&path).unwrap();
    let backend_before = latest_windows_for_backend(&before, "codex");
    let identity = ExecutionIdentity::legacy_candidate("codex", None::<&str>, None::<&str>);
    let identity_before = latest_for_identity(&before, &identity);

    let dropped = compact(
        &path,
        RetentionPolicy {
            max_per_window: 1,
            max_bytes: usize::MAX,
        },
        None,
    )
    .unwrap();
    assert!(dropped > 0, "compaction must actually drop something here");

    let after = load(&path).unwrap();
    assert_eq!(
        reading_summary(&latest_windows_for_backend(&after, "codex")),
        reading_summary(&backend_before),
        "the backend-wide selector must return the same readings after compaction"
    );
    assert_eq!(
        reading_summary(
            &latest_for_identity(&after, &identity)
                .into_iter()
                .collect::<Vec<_>>()
        ),
        reading_summary(&identity_before.into_iter().collect::<Vec<_>>()),
        "the identity selector must return the same reading after compaction"
    );
    // Exactly one observation per series survives: its newest.
    assert_eq!(after.len(), 3);
    assert_eq!(after[0].checked_at.as_deref(), Some(ts(2).as_str()));
    assert_eq!(after[1].checked_at.as_deref(), Some(ts(12).as_str()));
    assert_eq!(after[2].checked_at.as_deref(), Some(ts(22).as_str()));
}

/// #1539: credential lifecycle markers survive compaction even beyond the
/// per-window cap, so a rotated source's binding keeps resolving the same:
/// without the marker, the pre-rotation account's future-dated reading
/// would come back as current.
#[test]
fn compact_preserves_credential_lifecycle_markers() {
    let (_dir, path) = tmp_store();
    // Pre-rotation reading under the SAME labels the source will keep after
    // rotation, future-dated so freshness selection alone cannot save us.
    append(
        &path,
        &credential_observation(
            "cred-a",
            "mistral-dashboard:customer-two",
            Some("monthly"),
            &ts(90),
            Some(40.0),
            Some("mistral_dashboard"),
        ),
    )
    .unwrap();
    // Rotation marker.
    append(
        &path,
        &credential_observation(
            "cred-a",
            "mistral-dashboard:customer-two",
            None,
            &ts(1),
            None,
            Some("credential_updated"),
        ),
    )
    .unwrap();
    // Post-rotation reading.
    append(
        &path,
        &credential_observation(
            "cred-a",
            "mistral-dashboard:customer-two",
            Some("monthly"),
            &ts(2),
            Some(80.0),
            Some("mistral_dashboard"),
        ),
    )
    .unwrap();
    // Two newer window-less readings in the marker's series, so the marker
    // is beyond a cap of 2 unless lifecycle protection keeps it.
    for minute in 3..5 {
        append(
            &path,
            &credential_observation(
                "cred-a",
                "mistral-dashboard:customer-two",
                None,
                &ts(minute),
                Some(60.0),
                Some("mistral_dashboard"),
            ),
        )
        .unwrap();
    }

    let identity =
        ExecutionIdentity::legacy_candidate("mistral-dashboard", None::<&str>, None::<&str>);
    let before = load(&path).unwrap();
    let selected_before = reading_summary(&latest_windows_for_identity_and_credential(
        &before,
        &identity,
        Some("cred-a"),
    ));

    compact(
        &path,
        RetentionPolicy {
            max_per_window: 2,
            max_bytes: usize::MAX,
        },
        None,
    )
    .unwrap();

    let after = load(&path).unwrap();
    assert!(
        after.iter().any(
            |record| record.usage_source.as_deref() == Some("credential_updated")
                && record.checked_at.as_deref() == Some(ts(1).as_str())
        ),
        "the rotation marker must survive compaction"
    );
    let selected_after = reading_summary(&latest_windows_for_identity_and_credential(
        &after,
        &identity,
        Some("cred-a"),
    ));
    assert_eq!(
        selected_after, selected_before,
        "the credential selector must be unchanged by compaction"
    );
    assert!(
        !selected_before
            .iter()
            .any(|(checked_at, remaining)| *checked_at == Some(ts(90)) && *remaining == Some(40.0)),
        "the future-dated pre-rotation reading must stay excluded (sanity)"
    );
}

/// #1539: the size backstop trims the oldest history until the file fits,
/// but never a series' newest reading.
#[test]
fn compact_enforces_the_size_backstop_without_dropping_a_series_latest() {
    let (_dir, path) = tmp_store();
    for series in 0..5 {
        for step in 0..4 {
            append(
                &path,
                &observation(
                    "codex",
                    Some(&format!("codex-{series}")),
                    Some("five-hour"),
                    &ts(series * 10 + step),
                    (series * 10 + step) as f64,
                ),
            )
            .unwrap();
        }
    }
    let line_len = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .len()
        + 1;

    let policy = RetentionPolicy {
        max_per_window: 10,
        max_bytes: 4 * line_len,
    };
    let dropped = compact(&path, policy, None).unwrap();

    assert_eq!(dropped, 15, "only the newest reading of each series fits");
    let records = load(&path).unwrap();
    assert_eq!(records.len(), 5);
    for (series, record) in records.iter().enumerate() {
        assert_eq!(
            record.checked_at.as_deref(),
            Some(ts(series as i64 * 10 + 3).as_str()),
            "series {series} must keep its newest reading"
        );
    }
}

#[test]
fn compaction_due_only_past_the_size_bound() {
    let (_dir, path) = tmp_store();
    append(&path, &observation("codex", None, None, &ts(0), 50.0)).unwrap();
    let policy = RetentionPolicy {
        max_per_window: 500,
        max_bytes: 8 * 1024,
    };
    assert!(!compaction_due(&path, policy));
    assert!(compaction_due(
        &path,
        RetentionPolicy {
            max_per_window: 500,
            max_bytes: 8,
        }
    ));
}

/// #1539: the append site itself enforces the size bound — the file grows
/// every refresh, so bounding it only at prune time is not enough.
#[test]
fn appending_enforces_the_size_bound_at_the_write_site() {
    let _guard = crate::test_support::QuotaRetentionEnvGuard::set(usize::MAX, 8 * 1024);
    let (_dir, path) = tmp_store();
    for minute in 0..60 {
        append(
            &path,
            &observation("codex", None, Some("five-hour"), &ts(minute), 50.0),
        )
        .unwrap();
    }

    let policy = RetentionPolicy::from_env();
    assert!(
        !compaction_due(&path, policy),
        "the append site must have compacted the store back under the bound"
    );
    let lines = read_store_lines(&path).unwrap();
    assert!(
        lines.iter().all(|line| line.record.is_some()),
        "the compacted store must still parse line by line"
    );
    let records = load(&path).unwrap();
    assert!(
        !records
            .iter()
            .any(|record| record.checked_at.as_deref() == Some(ts(0).as_str())),
        "the oldest observation must be dropped"
    );
    assert_eq!(
        records.last().unwrap().checked_at.as_deref(),
        Some(ts(59).as_str()),
        "the newest observation must be kept"
    );
}

/// #1539: rotation (read-rewrite-rename) runs under the same lock as
/// appends, so concurrent writers can never lose a record or tear a line.
#[test]
fn rotation_never_loses_a_concurrent_append() {
    let (_dir, path) = tmp_store();
    let path = std::sync::Arc::new(path);
    let policy = RetentionPolicy {
        max_per_window: 5,
        max_bytes: usize::MAX,
    };
    let handles: Vec<_> = (0..4)
        .map(|series| {
            let path = std::sync::Arc::clone(&path);
            std::thread::spawn(move || {
                for step in 0..25 {
                    append(
                        &path,
                        &observation(
                            "codex",
                            Some(&format!("codex-{series}")),
                            Some("five-hour"),
                            &ts(series * 100 + step),
                            50.0,
                        ),
                    )
                    .unwrap();
                }
            })
        })
        .collect();
    // Rotate while the appenders run: a lost lock would show up as a torn
    // line or a missing newest reading below.
    for _ in 0..10 {
        compact(&path, policy, None).unwrap();
    }
    for handle in handles {
        handle.join().unwrap();
    }
    compact(&path, policy, None).unwrap();

    let lines = read_store_lines(&path).unwrap();
    assert!(
        lines.iter().all(|line| line.record.is_some()),
        "no line may be torn by a concurrent rotation"
    );
    let records = load(&path).unwrap();
    assert_eq!(records.len(), 20, "5 newest observations per series");
    for series in 0..4i64 {
        let series_records: Vec<_> = records
            .iter()
            .filter(|record| {
                record.backend_instance.as_deref() == Some(format!("codex-{series}").as_str())
            })
            .collect();
        let checked: Vec<_> = series_records
            .iter()
            .map(|record| record.checked_at.clone())
            .collect();
        assert_eq!(
            checked,
            (20..25)
                .map(|step| Some(ts(series * 100 + step)))
                .collect::<Vec<_>>(),
            "series {series} must hold exactly its five newest appends"
        );
    }
}
