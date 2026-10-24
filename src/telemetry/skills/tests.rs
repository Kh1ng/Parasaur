use super::super::*;
use crate::ledger::{
    skills::SkillVersion, test_util, AttemptRecord, AttemptRoutingRecord, LedgerUsage,
};

fn fixture() -> LedgerEntry {
    let mut entry = LedgerEntry::new(
        "test",
        &test_util::profile(),
        "codex",
        "improve",
        "task",
        None,
        None,
    );
    for (number, version, tokens) in [(1, "1", 10), (2, "2", 20)] {
        let skills = vec![SkillVersion {
            id: "review".into(),
            version: version.into(),
        }];
        entry.skills.extend(skills.clone());
        entry.attempt_routing.push(AttemptRoutingRecord {
            attempt_number: number,
            skills,
            ..Default::default()
        });
        entry.attempts.push(AttemptRecord {
            attempt_number: number,
            backend: "codex".into(),
            exit_code: Some(0),
            validation_result: Some("passed".into()),
            usage: LedgerUsage {
                total_tokens: Some(tokens),
                ..Default::default()
            },
            ..Default::default()
        });
    }
    entry
}

fn params(include_retried_attempts: bool) -> AggregationParams {
    AggregationParams {
        dimensions: vec![AggregationDimension::Skill],
        since: None,
        until: None,
        profile: None,
        include_failed_attempts: true,
        include_retried_attempts,
        project: None,
        ticket: None,
        execution_type: None,
        backend_instance: None,
        provider: None,
        model: None,
        account: None,
    }
}

#[test]
fn telemetry_skill_export_and_aggregation_preserve_versions() {
    let (tmp, cfg) = test_util::test_config();
    let entry = fixture();
    crate::ledger::append(&cfg, &entry).unwrap();
    let output = tmp.path().join("export");
    export_telemetry_from_config(
        &cfg,
        output.to_str(),
        exporter::ExportFormat::Jsonl,
        false,
        GroupBy::Skill,
        None,
        None,
    )
    .unwrap();
    let groups: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("skill-summary.json")).unwrap()).unwrap();
    assert_eq!(groups[0]["group_key"], "review@1");
    assert_eq!(groups[0]["total_tokens"], 10);
    assert_eq!(groups[1]["group_key"], "review@2");
    assert_eq!(groups[1]["total_tokens"], 20);
    let records = extractor::extract_attempt_usage_records(&entry, "2026-10-24T00:00:00Z");
    assert_eq!(records[0].skills[0].version, "1");
    assert_eq!(records[1].skills[0].version, "2");
    let mut legacy = serde_json::to_value(&records[0]).unwrap();
    legacy.as_object_mut().unwrap().remove("skills");
    assert!(
        serde_json::from_value::<records::AttemptUsageRecord>(legacy)
            .unwrap()
            .skills
            .is_empty()
    );
    let all = aggregate_by_dimension(
        std::slice::from_ref(&entry),
        AggregationDimension::Skill,
        &params(true),
    );
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].total_tokens, 10);
    assert_eq!(all[1].total_tokens, 20);
    let final_only = aggregate_by_dimension(&[entry], AggregationDimension::Skill, &params(false));
    assert_eq!(final_only.len(), 1);
    assert_eq!(final_only[0].dimension_value, "review@2");
    assert_eq!(final_only[0].total_tokens, 20);
}
