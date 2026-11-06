use super::*;
use crate::ledger::{
    append,
    test_util::{profile, test_config},
    AttemptRecord,
};

fn entry() -> LedgerEntry {
    let mut entry = LedgerEntry::new("test", &profile(), "codex", "fix", "task", None, None);
    entry.timestamp = "2026-11-06T10:00:00Z".into();
    entry.work_id = Some("#42".into());
    entry
}

#[test]
fn legacy_summary_readings_survive_when_attempt_usage_was_never_recorded() {
    let _clock = crate::clock::set_for_test(
        OffsetDateTime::parse("2026-11-06T12:00:00Z", &Rfc3339).unwrap(),
    );
    let (_tmp, cfg) = test_config();
    let mut entry = entry();
    entry.schema_version = 9;
    entry.usage.total_tokens = Some(70);
    entry.attempts = vec![
        AttemptRecord {
            backend: "codex".into(),
            attempt_number: 1,
            ..Default::default()
        },
        AttemptRecord {
            backend: "codex".into(),
            attempt_number: 2,
            ..Default::default()
        },
    ];
    append(&cfg, &entry).unwrap();
    let row = build(&cfg, "7d", None).unwrap().rows.pop().unwrap();
    assert_eq!(row.dispatch_attempts, 2);
    assert_eq!(row.total_tokens, Some(70));
    assert_eq!(row.legacy_dispatches, 1);
}

#[test]
fn unstarted_marker_cannot_hide_execution_result_evidence() {
    let mut entry = entry();
    entry.usage.usage_unknown_reason = Some(UsageUnknownReason::NoAttemptStarted);
    assert!(proven_unstarted(&entry));
    entry.review_verdict = Some("APPROVE".into());
    assert!(!proven_unstarted(&entry));
    entry.review_verdict = None;
    entry.validation_result = Some("passed".into());
    assert!(!proven_unstarted(&entry));
    entry.validation_result = None;
    entry.usage.total_tokens = Some(0);
    assert!(!proven_unstarted(&entry));
}

#[test]
fn attempt_readings_supersede_mirrored_dispatch_usage_and_preserve_identity() {
    let _clock = crate::clock::set_for_test(
        OffsetDateTime::parse("2026-11-06T12:00:00Z", &Rfc3339).unwrap(),
    );
    let (_tmp, cfg) = test_config();
    let mut entry = entry();
    entry.usage.total_tokens = Some(999);
    entry.usage.actual_cost_usd = Some(999.0);
    entry.attempts = [10, 20]
        .into_iter()
        .enumerate()
        .map(|(index, tokens)| AttemptRecord {
            attempt_number: index as u32 + 1,
            backend: "codex".into(),
            effective_model: Some("route-alias".into()),
            usage: LedgerUsage {
                backend_instance: Some("work".into()),
                actual_model: Some("reported-model".into()),
                total_tokens: Some(tokens),
                cache_read_tokens: Some(80),
                actual_cost_usd: Some(0.0),
                estimated_cost_usd: Some(0.25),
                ..Default::default()
            },
            ..Default::default()
        })
        .collect();
    append(&cfg, &entry).unwrap();
    let report = build(&cfg, "7d", Some("test")).unwrap();
    assert_eq!(report.rows.len(), 1);
    let row = &report.rows[0];
    assert_eq!(row.dispatch_attempts, 2);
    assert_eq!(row.legacy_dispatches, 0);
    assert_eq!(row.total_tokens, Some(30));
    assert_eq!(row.cache_read_tokens, Some(160));
    assert_eq!(row.input_tokens, None);
    assert_eq!(row.actual_cost_usd, Some(0.0));
    assert_eq!(row.estimated_cost_usd, Some(0.5));
    assert_eq!(row.identity.backend_instance.as_deref(), Some("work"));
    assert_eq!(row.identity.actual_model.as_deref(), Some("reported-model"));
    assert_eq!(row.identity.effective_model.as_deref(), Some("route-alias"));
    assert_eq!(row.identity.requested_model, None);
}

#[test]
fn missing_attempt_usage_does_not_fall_back_to_dispatch_totals_or_zero() {
    let _clock = crate::clock::set_for_test(
        OffsetDateTime::parse("2026-11-06T12:00:00Z", &Rfc3339).unwrap(),
    );
    let (_tmp, cfg) = test_config();
    let mut entry = entry();
    entry.usage.total_tokens = Some(999);
    entry.attempts = vec![AttemptRecord {
        backend: "codex".into(),
        attempt_number: 1,
        ..Default::default()
    }];
    append(&cfg, &entry).unwrap();
    let row = build(&cfg, "7d", None).unwrap().rows.pop().unwrap();
    assert_eq!(row.dispatch_attempts, 1);
    assert_eq!(row.total_tokens, None);
    assert_eq!(row.actual_cost_usd, None);
}

#[test]
fn time_scope_preflight_proof_legacy_unknowns_and_failed_reads_remain_distinct() {
    let _clock = crate::clock::set_for_test(
        OffsetDateTime::parse("2026-11-06T12:00:00Z", &Rfc3339).unwrap(),
    );
    let (_tmp, cfg) = test_config();
    let unstarted = entry();
    append(&cfg, &unstarted).unwrap();
    let mut legacy = entry();
    legacy.attempts_started = None;
    legacy.attempts_completed = None;
    legacy.timestamp = "2026-11-06T00:30:00+01:00".into();
    append(&cfg, &legacy).unwrap();
    let mut future = legacy.clone();
    future.timestamp = "2026-11-06T13:00:00Z".into();
    append(&cfg, &future).unwrap();
    let mut contradiction = entry();
    contradiction.validation_result = Some("deferred_capacity".into());
    contradiction.usage.usage_unknown_reason = Some(UsageUnknownReason::NoAttemptStarted);
    contradiction.usage.total_tokens = Some(0);
    append(&cfg, &contradiction).unwrap();
    let report = build(&cfg, "7d", Some("test")).unwrap();
    assert_eq!(report.excluded_preflight_entries, 1);
    assert_eq!(report.entries, 2);
    assert_eq!(report.rows.len(), 2);
    assert_eq!(
        report
            .rows
            .iter()
            .find(|row| row.identity.day == "2026-11-05")
            .unwrap()
            .total_tokens,
        None
    );
    assert_eq!(
        report
            .rows
            .iter()
            .find(|row| row.identity.day == "2026-11-06")
            .unwrap()
            .total_tokens,
        Some(0)
    );
    let mut invalid = legacy;
    invalid.timestamp = "invalid".into();
    append(&cfg, &invalid).unwrap();
    assert!(build(&cfg, "7d", Some("test"))
        .unwrap_err()
        .to_string()
        .contains("invalid ledger timestamp"));
}
