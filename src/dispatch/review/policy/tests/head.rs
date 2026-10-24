use super::*;

#[test]
fn escalation_is_spent_for_same_head_after_metadata_edit_and_resets_on_push() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = gah_config_with_ledger(tmp.path(), RoutingPolicy::default());
    let prof = profile(tmp.path());
    let entry = review_ledger_entry("test", &prof, "gah/1617", "HUMAN_REVIEW", "low");
    crate::ledger::append(&cfg, &entry).unwrap();
    assert_eq!(
        super::super::review_escalation_reason(
            &cfg,
            &prof,
            "test",
            "gah/1617",
            Some("review-v1:reviewed-sha:sha256:edited")
        ),
        Some("human_review")
    );
    assert_eq!(
        super::super::review_escalation_reason(
            &cfg,
            &prof,
            "test",
            "gah/1617",
            Some("review-v1:new-sha:sha256:edited")
        ),
        None
    );
}

#[test]
fn escalatory_route_is_tried_once_per_head_despite_metadata_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = gah_config_with_ledger(tmp.path(), RoutingPolicy::default());
    let mut prof = profile(tmp.path());
    prof.routing.escalatory_reviewers = vec![CandidateConfig {
        backend: "codex".into(),
        model: Some("review-model".into()),
        ..Default::default()
    }];
    let mut entry = review_ledger_entry("test", &prof, "gah/1617", "review_output_invalid", "low");
    entry.effective_backend = "codex".into();
    entry.effective_model = Some("review-model".into());
    crate::ledger::append(&cfg, &entry).unwrap();
    let next = |generation| {
        super::super::next_escalatory_reviewer(
            &cfg,
            &prof,
            "test",
            "gah/1617",
            None,
            Some(generation),
            &tmp.path().join("availability.json"),
        )
    };
    assert!(next("review-v1:reviewed-sha:sha256:edited").is_none());
    assert!(next("review-v1:new-sha:sha256:edited").is_some());
}

#[test]
fn review_ci_only_needs_fix_waits_for_push_but_mixed_or_unconfirmed_findings_are_invalid() {
    let bundle = ReviewDiffBundle {
        files: "src/lib.rs\n".into(),
        diff: "+fn example() {}\n".into(),
    };
    let json = r#"{"verdict":"NEEDS_FIX","confidence":"low","human_required":false,
        "blocking_findings":["ci:failed:rust"],"actionable_findings":[],
        "non_blocking_findings":[],"risk_notes":[],"evidence":[]}"#;
    let parse = |text: &str, ci| {
        parse_review_verdict_with_context(
            text,
            &route_decision("codex", None, false),
            &crate::ledger::LedgerUsage::default(),
            ReviewerTier::Strong,
            &ReviewGateContext::from_diff_bundle(&bundle, Some(ci)),
        )
    };
    let verdict = parse(json, "failed").unwrap();
    assert_eq!(verdict.verdict, "NEEDS_FIX");
    assert!(!verdict.human_required);
    assert_eq!(
        verdict.safety_gate_reason.as_deref(),
        Some("ci_failed_waiting_for_push")
    );
    assert!(verdict.actionable_findings.is_empty());
    for status in ["success", "running", "unknown"] {
        assert!(parse(json, status).is_err());
    }
    assert!(parse(
        &json.replace("ci:failed:rust", "src/lib.rs: broken code"),
        "failed"
    )
    .is_err());
    assert!(parse(&json.replace("ci:failed:rust", "ci:failed:"), "failed").is_err());
}
