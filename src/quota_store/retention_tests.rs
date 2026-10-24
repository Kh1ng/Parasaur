use super::*;
use crate::execution_identity::ExecutionIdentity;
use crate::quota_store::{append, load};

fn observation(
    credential: Option<&str>,
    window: Option<&str>,
    minute: i64,
) -> QuotaObservationRecord {
    let timestamp = (OffsetDateTime::parse("2026-01-01T00:00:00Z", &Rfc3339).unwrap()
        + Duration::minutes(minute))
    .format(&Rfc3339)
    .unwrap();
    QuotaObservationRecord {
        backend: "codex".into(),
        credential_id: credential.map(str::to_string),
        backend_instance: Some("account".into()),
        quota_pool: Some("account".into()),
        model: None,
        quota_window: window.map(str::to_string),
        quota_remaining_percent: Some(50.0),
        quota_reset_at: None,
        observed_at: Some(timestamp.clone()),
        checked_at: Some(timestamp),
        check_error: None,
        usage_source: Some("test_refresh".into()),
        account_usage: None,
    }
}

fn marker(credential: &str, minute: i64, source: &str) -> QuotaObservationRecord {
    QuotaObservationRecord {
        usage_source: Some(source.into()),
        quota_remaining_percent: None,
        ..observation(Some(credential), None, minute)
    }
}

fn compact_fixture(
    records: &[QuotaObservationRecord],
    cutoff: OffsetDateTime,
) -> Vec<QuotaObservationRecord> {
    let keep = plan_retention(records, cutoff);
    records
        .iter()
        .enumerate()
        .filter(|(index, _)| keep.contains(index))
        .map(|(_, record)| record.clone())
        .collect()
}

// Compare complete records from every public selector, including credential,
// explicit-instance and model scopes, rather than only timestamps/percentages.
fn assert_equivalent(before: &[QuotaObservationRecord], after: &[QuotaObservationRecord]) {
    use crate::quota_store::*;
    let json = |records: Vec<&QuotaObservationRecord>| serde_json::to_value(records).unwrap();
    assert_eq!(
        json(latest_windows_for_backend(before, "codex")),
        json(latest_windows_for_backend(after, "codex"))
    );
    for model in [None, Some("model-a"), Some("model-b")] {
        for explicit in [false, true] {
            let mut identity = ExecutionIdentity::legacy_candidate("codex", model, None::<&str>);
            identity.backend_instance = "account".into();
            identity.quota_pool = Some("account".into());
            identity.explicit_instance = explicit;
            assert_eq!(
                json(latest_windows_for_identity(before, &identity)),
                json(latest_windows_for_identity(after, &identity))
            );
            assert_eq!(
                serde_json::to_value(latest_for_identity(before, &identity)).unwrap(),
                serde_json::to_value(latest_for_identity(after, &identity)).unwrap()
            );
            for credential in [None, Some("cred-a"), Some("cred-b")] {
                assert_eq!(
                    json(latest_windows_for_identity_and_credential(
                        before, &identity, credential
                    )),
                    json(latest_windows_for_identity_and_credential(
                        after, &identity, credential
                    ))
                );
            }
        }
    }
}

#[test]
fn keeps_recent_history_and_newest_old_window_without_a_count_or_byte_gate() {
    let now = OffsetDateTime::now_utc();
    let mut records: Vec<_> = (0..600)
        .map(|minute| observation(None, Some("weekly"), minute))
        .collect();
    let mut recent = observation(None, Some("weekly"), 0);
    recent.checked_at = Some(now.format(&Rfc3339).unwrap());
    recent.observed_at = recent.checked_at.clone();
    records.extend((0..600).map(|_| recent.clone()));
    records.push(observation(None, Some("five-hour"), 1));
    let after = compact_fixture(&records, now - RetentionPolicy::default().max_age);
    assert_eq!(
        after.len(),
        601,
        "all recent readings and each old window's newest survive"
    );
    assert_equivalent(&records, &after);
}

#[test]
fn rotation_keeps_current_windows_despite_future_dated_retired_readings() {
    let records = vec![
        observation(Some("cred-a"), Some("weekly"), 59),
        marker("cred-a", 0, "credential_updated"),
        marker("cred-a", 1, "credential_updated"),
        observation(Some("cred-a"), Some("weekly"), 2),
        observation(Some("cred-a"), Some("five-hour"), 1),
        observation(Some("cred-b"), Some("weekly"), 4),
    ];
    let after = compact_fixture(&records, OffsetDateTime::now_utc());
    assert_eq!(
        after.len(),
        4,
        "only the latest lifecycle marker and current windows are dependencies"
    );
    assert_equivalent(&records, &after);
    let removed = [records, vec![marker("cred-a", 0, "credential_removed")]].concat();
    let after = compact_fixture(&removed, OffsetDateTime::now_utc());
    assert_equivalent(&removed, &after);
    assert!(after
        .iter()
        .any(|record| record.usage_source.as_deref() == Some("credential_removed")));
}

#[test]
fn last_append_binding_survives_account_changes_and_clock_rollback() {
    let first = observation(Some("cred-a"), Some("weekly"), 90);
    let fresh = observation(Some("cred-a"), Some("five-hour"), 100);
    let mut other = observation(Some("cred-a"), Some("weekly"), 1);
    other.backend_instance = Some("other-account".into());
    other.checked_at = Some(OffsetDateTime::now_utc().format(&Rfc3339).unwrap());
    let mut binding = observation(Some("cred-a"), Some("five-hour"), 0);
    binding.quota_remaining_percent = Some(40.0);
    let records = vec![first, fresh, other, binding];
    let after = compact_fixture(&records, OffsetDateTime::now_utc() - Duration::days(1));
    assert_eq!(
        after.len(),
        4,
        "the old binding append must survive despite newer readings"
    );
    assert_equivalent(&records, &after);
}

#[test]
fn failures_cannot_revive_windows_after_later_successes_or_timestamp_ties() {
    for window in [None, Some("weekly")] {
        for equal_time in [false, true] {
            for model in [None, Some("model-a"), Some("model-b")] {
                let success = observation(Some("cred-a"), Some("weekly"), 1);
                let mut failure = observation(Some("cred-a"), window, 2);
                failure.check_error = Some("failed".into());
                failure.model = model.map(str::to_string);
                let later = observation(Some("cred-a"), window, if equal_time { 2 } else { 3 });
                let records = vec![
                    success,
                    failure,
                    later,
                    observation(Some("cred-b"), Some("weekly"), 1),
                ];
                let after = compact_fixture(&records, OffsetDateTime::now_utc());
                assert_equivalent(&records, &after);
                assert!(after.iter().any(|record| record.check_error.is_some()));
            }
        }
    }
    let mut untimed = observation(None, Some("weekly"), 1);
    untimed.checked_at = None;
    untimed.observed_at = None;
    let mut failure = untimed.clone();
    failure.check_error = Some("untimed failure".into());
    let records = vec![untimed, failure];
    assert_equivalent(
        &records,
        &compact_fixture(&records, OffsetDateTime::now_utc()),
    );
}

#[test]
fn failed_export_leaves_store_byte_identical_and_append_never_deletes_history() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("quota.jsonl");
    for minute in 0..600 {
        append(&path, &observation(None, Some("weekly"), minute)).unwrap();
    }
    let before = fs::read(&path).unwrap();
    assert_eq!(load(&path).unwrap().len(), 600);
    assert!(
        compact_with_export(&path, RetentionPolicy::default(), |_| anyhow::bail!(
            "export failed"
        ))
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let blocked = root.path().join("not-a-directory");
    fs::write(&blocked, "blocked").unwrap();
    let cfg = crate::config::GahConfig {
        defaults: crate::config::Defaults {
            artifact_root: blocked.to_string_lossy().into_owned(),
            ..Default::default()
        },
        profiles: Default::default(),
        context: Default::default(),
    };
    assert!(compact(&path, RetentionPolicy::default(), &cfg).is_err());
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "real exporter failure also aborts replacement"
    );
}

#[test]
fn export_and_replace_share_one_snapshot_while_appends_wait() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("quota.jsonl");
    for minute in 0..4 {
        append(&path, &observation(None, Some("weekly"), minute)).unwrap();
    }
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let writer_path = path.clone();
    let mut writer = None;
    compact_with_export(&path, RetentionPolicy::default(), |snapshot| {
        assert_eq!(snapshot.len(), 4);
        writer = Some(std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            append(&writer_path, &observation(None, Some("weekly"), 4)).unwrap();
            done_tx.send(()).unwrap();
        }));
        started_rx.recv().unwrap();
        assert!(done_rx
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err());
        let probe = OpenOptions::new()
            .write(true)
            .open(lock_path_for(&path))
            .unwrap();
        assert!(
            probe.try_lock_exclusive().is_err(),
            "export holds the append lock"
        );
        Ok(())
    })
    .unwrap();
    writer.unwrap().join().unwrap();
    let after = load(&path).unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!(
        after[1].checked_at,
        observation(None, Some("weekly"), 4).checked_at
    );
}

#[test]
fn rotation_never_loses_a_concurrent_append() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("quota.jsonl");
    for step in 0..2 {
        append(&path, &observation(None, Some("history"), step)).unwrap();
    }
    let writers: Vec<_> = (0..4)
        .map(|writer| {
            let path = path.clone();
            std::thread::spawn(move || {
                for step in 0..100 {
                    // Each append has its own protected window. Losing any old inode
                    // write therefore loses a required winner, not disposable history.
                    crate::quota_store::append_all(
                        &path,
                        &[
                            observation(None, Some(&format!("{writer}-{step}")), step),
                            observation(None, Some("history"), step),
                        ],
                    )
                    .unwrap();
                }
            })
        })
        .collect();
    let mut dropped = 0;
    for _ in 0..10 {
        dropped += compact_with_export(&path, RetentionPolicy::default(), |_| Ok(())).unwrap();
    }
    for writer in writers {
        writer.join().unwrap();
    }
    compact_with_export(&path, RetentionPolicy::default(), |_| Ok(())).unwrap();
    assert!(dropped > 0, "exercise replacement while appenders run");
    assert_eq!(load(&path).unwrap().len(), 401);
    assert!(read_store_lines(&path)
        .unwrap()
        .iter()
        .all(|line| line.record.is_some()));
}
