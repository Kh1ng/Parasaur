//! Prune integration coverage for global quota and availability retention.
use super::prune_global_state_at;
use crate::config::{Defaults, GahConfig};
use std::collections::HashMap;
use std::fs;

fn rfc3339(moment: time::OffsetDateTime) -> String {
    moment
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

fn telemetry_quota_export_text(telemetry_root: &std::path::Path) -> String {
    let mut text = String::new();
    let mut pending = vec![telemetry_root.join("raw").join("quota")];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "jsonl") {
                text.push_str(&fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }
    text
}

/// Prune applies age retention even to small stores, exporting the exact
/// snapshot before deletion, and drops superseded expired availability.
#[test]
fn prune_compacts_global_quota_and_availability_state() {
    let root = tempfile::tempdir().unwrap();
    let store_path = root.path().join("quota_observations.jsonl");
    let availability_path = root.path().join("availability.json");

    let base = time::OffsetDateTime::parse(
        "2026-10-20T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let observation = |minute: i64| crate::quota_store::QuotaObservationRecord {
        backend: "codex".to_string(),
        credential_id: None,
        backend_instance: None,
        model: None,
        quota_pool: None,
        quota_window: Some("five-hour".to_string()),
        quota_remaining_percent: Some(50.0),
        quota_reset_at: None,
        observed_at: Some(rfc3339(base + time::Duration::minutes(minute))),
        checked_at: Some(rfc3339(base + time::Duration::minutes(minute))),
        check_error: None,
        usage_source: Some("test_refresh".to_string()),
        account_usage: None,
    };
    for minute in 0..8 {
        crate::quota_store::append(&store_path, &observation(minute)).unwrap();
    }

    let now = time::OffsetDateTime::now_utc();
    crate::availability::record_unavailable(
        &availability_path,
        "claude",
        None,
        None,
        crate::availability::Reason::RateLimited,
        crate::availability::Source::BackendError,
        Some(now - time::Duration::hours(1)),
        None,
        now - time::Duration::hours(2),
    )
    .unwrap();
    let expired = crate::availability::load_state(&availability_path)
        .unwrap()
        .records[0]
        .clone();
    crate::availability::record_available(
        &availability_path,
        "claude",
        None,
        None,
        crate::availability::Source::Manual,
        now,
    )
    .unwrap();

    // Seed directly: update_state already removed the expired predecessor.
    let mut state = crate::availability::load_state(&availability_path).unwrap();
    state.records.insert(0, expired);
    fs::write(&availability_path, serde_json::to_vec(&state).unwrap()).unwrap();
    let cfg = GahConfig {
        defaults: Defaults {
            artifact_root: root.path().to_string_lossy().to_string(),
            ..Default::default()
        },
        profiles: HashMap::new(),
        context: Default::default(),
    };
    let policy = crate::quota_store::RetentionPolicy {
        max_age: time::Duration::ZERO,
    };

    prune_global_state_at(&cfg, &availability_path, &store_path, policy, false).unwrap();

    // Old observations dropped, the newest kept.
    let records = crate::quota_store::load(&store_path).unwrap();
    assert!(!records.is_empty());
    assert!(
        records
            .iter()
            .all(|record| record.checked_at.as_deref() != Some(rfc3339(base).as_str())),
        "the oldest observations must be compacted away"
    );
    assert_eq!(
        records.last().unwrap().checked_at.as_deref(),
        Some(rfc3339(base + time::Duration::minutes(7)).as_str()),
        "the newest observation must be kept"
    );

    // Availability: the superseded expired entry is dropped, the latest stays.
    let state = crate::availability::load_state(&availability_path).unwrap();
    assert_eq!(state.records.len(), 1);
    assert_eq!(
        state.records[0].status,
        crate::availability::Status::Available
    );

    // #1539 acceptance: the telemetry export ran BEFORE compaction — the
    // oldest observation is gone from the store but present in the export.
    let exported = telemetry_quota_export_text(&root.path().join("telemetry"));
    assert!(
        exported.contains(&rfc3339(base)),
        "records dropped by compaction must already be exported"
    );
}

/// #1539: --dry-run never rewrites the global state files.
#[test]
fn prune_dry_run_never_compacts_global_state() {
    let root = tempfile::tempdir().unwrap();
    let store_path = root.path().join("quota_observations.jsonl");
    let availability_path = root.path().join("availability.json");
    let base = time::OffsetDateTime::parse(
        "2026-10-20T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    for minute in 0..8 {
        let record = crate::quota_store::QuotaObservationRecord {
            observed_at: Some(rfc3339(base + time::Duration::minutes(minute))),
            checked_at: Some(rfc3339(base + time::Duration::minutes(minute))),
            backend: "codex".to_string(),
            credential_id: None,
            backend_instance: None,
            model: None,
            quota_pool: None,
            quota_window: Some("five-hour".to_string()),
            quota_remaining_percent: Some(50.0),
            quota_reset_at: None,
            check_error: None,
            usage_source: Some("test_refresh".to_string()),
            account_usage: None,
        };
        crate::quota_store::append(&store_path, &record).unwrap();
    }
    let before = fs::read_to_string(&store_path).unwrap();
    let availability_before = r#"{"version":2,"records":[]}"#;
    fs::write(&availability_path, availability_before).unwrap();
    let telemetry = root.path().join("telemetry");

    let cfg = GahConfig {
        defaults: Defaults {
            artifact_root: root.path().to_string_lossy().into_owned(),
            ..Default::default()
        },
        profiles: HashMap::new(),
        context: Default::default(),
    };
    prune_global_state_at(
        &cfg,
        &availability_path,
        &store_path,
        crate::quota_store::RetentionPolicy {
            max_age: time::Duration::ZERO,
        },
        true,
    )
    .unwrap();

    assert_eq!(
        fs::read_to_string(&store_path).unwrap(),
        before,
        "a dry run must not rewrite the store"
    );
    assert_eq!(
        fs::read_to_string(&availability_path).unwrap(),
        availability_before
    );
    assert!(!root.path().join("availability.json.lock").exists());
    assert!(!telemetry.exists());
}
