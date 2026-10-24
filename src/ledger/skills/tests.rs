use super::*;
use crate::ledger::{test_util, AttemptRecord, AttemptRoutingRecord, LedgerUsage};

fn entry() -> LedgerEntry {
    LedgerEntry::new(
        "test",
        &test_util::profile(),
        "codex",
        "improve",
        "task",
        None,
        None,
    )
}

fn resolved(id: &str, version: &str) -> ResolvedSkill {
    ResolvedSkill {
        id: id.into(),
        version: version.into(),
        content: "secret instructions".into(),
        backends: vec![],
    }
}

#[test]
fn ledger_skill_round_trip_and_legacy_defaults() {
    let mut entry = entry();
    entry.attempt_routing.push(AttemptRoutingRecord {
        attempt_number: 1,
        ..Default::default()
    });
    entry.record_skills(&[resolved("review", "1.0.0")]);
    entry.attempt_routing.push(AttemptRoutingRecord {
        attempt_number: 2,
        ..Default::default()
    });
    entry.record_skills(&[resolved("review", "2.0.0")]);
    let mut json = serde_json::to_value(&entry).unwrap();
    assert_eq!(json["schema_version"], 11);
    assert_eq!(json["skills"].as_array().unwrap().len(), 2);
    assert!(!json.to_string().contains("secret instructions"));
    let loaded: LedgerEntry = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(loaded.skills_for_attempt(1)[0].version, "1.0.0");
    assert_eq!(loaded.skills_for_attempt(2)[0].version, "2.0.0");
    assert!(loaded.skills_for_attempt(3).is_empty());
    json.as_object_mut().unwrap().remove("skills");
    for route in json["attempt_routing"].as_array_mut().unwrap() {
        route.as_object_mut().unwrap().remove("skills");
    }
    json["schema_version"] = serde_json::json!(10);
    let old: LedgerEntry = serde_json::from_value(json).unwrap();
    assert!(old.skills.is_empty());
    assert!(old.skills_for_attempt(1).is_empty());
}

#[test]
fn ledger_rejects_malformed_skill_records() {
    for malformed in [
        serde_json::json!({"id":"a"}),
        serde_json::json!({"id":4,"version":"1"}),
        serde_json::json!({"id":"a","version":""}),
    ] {
        let mut json = serde_json::to_value(entry()).unwrap();
        json["skills"] = serde_json::json!([malformed]);
        assert!(serde_json::from_value::<LedgerEntry>(json).is_err());
    }
}

#[test]
fn skill_report_separates_attempt_usage_versions_and_unknowns() {
    let mut entry = entry();
    for (number, version, tokens) in [(1, "1.0.0", 10), (2, "2.0.0", 20), (3, "", 30)] {
        entry.attempt_routing.push(AttemptRoutingRecord {
            attempt_number: number,
            ..Default::default()
        });
        if !version.is_empty() {
            entry.record_skills(&[resolved("review", version), resolved("shared", "1")]);
        }
        entry.attempts.push(AttemptRecord {
            attempt_number: number,
            backend: "codex".into(),
            exit_code: Some(0),
            usage: LedgerUsage {
                total_tokens: Some(tokens),
                ..Default::default()
            },
            ..Default::default()
        });
    }
    entry.usage.total_tokens = Some(999);
    let groups = summarize(&[entry]);
    let group = |key| groups.iter().find(|g| g.group_key == key).unwrap();
    assert_eq!(group("review@1.0.0").total_tokens, Some(10));
    assert_eq!(group("review@2.0.0").total_tokens, Some(20));
    assert_eq!(group("shared@1").total_tokens, Some(30));
    assert_eq!(group("shared@1").attempts, 2);
    assert_eq!(group("shared@1").entries, 1);
    assert_eq!(group("unknown").total_tokens, Some(30));
}

#[test]
fn skill_report_keeps_unknown_counters_and_latest_attempt_outcome() {
    let mut entry = entry();
    entry.attempts_started = None;
    entry.attempts_completed = None;
    for number in [1, 2] {
        entry.attempt_routing.push(AttemptRoutingRecord {
            attempt_number: number,
            ..Default::default()
        });
        entry.record_skills(&[resolved("review", "1")]);
        entry.attempts.push(AttemptRecord {
            attempt_number: number,
            backend: "codex".into(),
            exit_code: Some(if number == 1 { 0 } else { 1 }),
            validation_result: (number == 1).then(|| "passed".into()),
            ..Default::default()
        });
    }
    let groups = summarize(&[entry]);
    assert_eq!(groups[0].attempts_started, None);
    assert_eq!(groups[0].attempts_completed, None);
    assert_eq!(groups[0].validation_pass, 0);
}

#[test]
fn ledger_skill_schema_bump_preserves_v10_usage_normalization() {
    let mut entry = entry();
    entry.schema_version = 10;
    entry.attempts_started = Some(1);
    entry.attempts_completed = Some(1);
    entry.backend_exit_code = Some(0);
    assert_eq!(
        entry
            .normalized_for_persistence()
            .usage
            .usage_unknown_reason,
        Some(crate::ledger::UsageUnknownReason::UsageArtifactMissing)
    );
}
