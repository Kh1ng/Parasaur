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

#[test]
fn a_route_is_excluded_by_its_failure_only_until_it_delivers_the_work() {
    use std::collections::HashSet;
    let tmp = tempfile::tempdir().unwrap();
    let prof = profile(tmp.path());
    let route = |model: &str| CandidateIdentity::new("codex", Some(model));
    let attempt = |number: u32, model: &str, failure: Option<&str>| crate::ledger::AttemptRecord {
        attempt_number: number,
        backend: "codex".into(),
        effective_model: Some(model.into()),
        failure_class: failure.map(str::to_string),
        ..Default::default()
    };
    let dispatch = |attempts: Vec<crate::ledger::AttemptRecord>, delivered: bool| {
        let mut entry = LedgerEntry::new("test", &prof, "codex", "fix", "x", None, None);
        entry.work_id = Some("ISSUE-42".into());
        entry.effective_backend = "codex".into();
        entry.effective_model = Some("worker-model".into());
        entry.attempts = attempts;
        if delivered {
            entry.commit_created = true;
            entry.validation_result = Some("passed".into());
        }
        entry
    };
    let mut current = LedgerEntry::new("test", &prof, "auto", "fix", "x", None, None);
    current.work_id = Some("ISSUE-42".into());
    let excluded =
        |history: &[LedgerEntry]| routing_runtime_state_from_entries(history, &current).attempted;

    // One dispatch: the first attempt changed nothing, the second produced
    // the change that passed validation. A later repair of the same work
    // must still be able to use that route; with one route it had nowhere
    // else to go and was deferred for ever.
    let delivered = dispatch(
        vec![
            attempt(1, "worker-model", Some("agent_no_progress")),
            attempt(2, "worker-model", None),
        ],
        true,
    );
    assert!(excluded(std::slice::from_ref(&delivered)).is_empty());

    // The same when the validated change then failed to publish: that
    // failure is the harness's, not the route's.
    let mut unpublished = delivered.clone();
    unpublished.failure_class = Some("harness_error".into());
    assert!(excluded(&[unpublished]).is_empty());

    // A dispatch that never delivered excludes the route, before or after
    // a delivery; only a delivery that comes later lifts it.
    let mut failed = dispatch(
        vec![
            attempt(1, "worker-model", Some("agent_failure")),
            attempt(2, "worker-model", Some("agent_no_progress")),
        ],
        false,
    );
    failed.failure_class = Some("agent_no_progress".into());
    let only_worker = HashSet::from([route("worker-model")]);
    assert_eq!(excluded(std::slice::from_ref(&failed)), only_worker);
    assert_eq!(excluded(&[delivered.clone(), failed.clone()]), only_worker);
    assert!(excluded(&[failed, delivered.clone()]).is_empty());

    // Anything short of a committed change that passed validation is not a
    // delivery, and neither is a dispatch that ended in the agent's failure.
    let mut unvalidated = delivered.clone();
    unvalidated.validation_result = Some("failed".into());
    assert_eq!(excluded(&[unvalidated]), only_worker);
    let mut uncommitted = delivered.clone();
    uncommitted.commit_created = false;
    assert_eq!(excluded(&[uncommitted]), only_worker);
    let mut ended_badly = delivered.clone();
    ended_badly.failure_class = Some("agent_failure".into());
    assert_eq!(excluded(&[ended_badly]), only_worker);
    let mut last_attempt_failed = delivered;
    last_attempt_failed.attempts[1].failure_class = Some("context_limit_exceeded".into());
    assert_eq!(excluded(&[last_attempt_failed]), only_worker);

    // Only the route that delivered is cleared. Another route that failed
    // earlier in the same dispatch stays excluded.
    let handed_over = dispatch(
        vec![
            attempt(1, "weak-model", Some("agent_failure")),
            attempt(2, "worker-model", None),
        ],
        true,
    );
    assert_eq!(
        excluded(&[handed_over]),
        HashSet::from([route("weak-model")])
    );
}
