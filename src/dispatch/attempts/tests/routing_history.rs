use super::*;

#[test]
fn routing_history_discards_empty_review_refusals_without_changing_metrics() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = gah_config_with_ledger(tmp.path(), RoutingPolicy::default());
    let prof = profile(tmp.path());
    let current = LedgerEntry::new("test", &prof, "codex", "fix", "fixture", None, None);
    let mut refused = LedgerEntry::new("test", &prof, "claude", "review", "fixture", None, None);
    refused.validation_result = Some("deferred_capacity".into());
    refused.error_summary = Some("x".repeat(1024 * 1024));
    let mut budget = refused.clone();
    budget.validation_result = Some("review_budget_exhausted".into());
    let mut prior_call = refused.clone();
    prior_call.backend_exit_code = Some(0);
    let mut counted_call = refused.clone();
    counted_call.attempts_completed = Some(1);
    let mut verdict = refused.clone();
    verdict.review_verdict = Some("APPROVE".into());
    let mut repair = refused.clone();
    repair.dispatch_reason = Some("post_review_repair".into());
    let mut approval = refused.clone();
    approval.mode = "paid_route_approval_grant".into();
    approval.work_id = Some("#1".into());
    let mut current = current;
    current.work_id = Some("#1".into());
    let entries = [
        refused,
        budget,
        prior_call,
        counted_call,
        verdict,
        repair,
        approval,
    ];
    std::fs::write(
        cfg.defaults.ledger_path(),
        entries
            .iter()
            .map(|entry| serde_json::to_string(entry).unwrap())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let retained = read_routing_history(&cfg, "test").unwrap();
    assert_eq!(
        retained.len(),
        5,
        "only empty review refusals are discarded"
    );
    let expected = routing_runtime_state_from_entries(&entries, &current);
    let actual = routing_runtime_state(&cfg, &current).unwrap();
    assert_eq!(actual.recent_runs, expected.recent_runs);
    assert_eq!(actual.attempted, expected.attempted);
    assert_eq!(actual.approved, expected.approved);
    assert_eq!(actual.dispatch_attempted, expected.dispatch_attempted);
    assert_eq!(
        actual.reviewer_outcomes,
        crate::routing::reviewer_outcome_metrics(&entries, &[], "test", &prof.repo_id)
    );
}
