use super::*;

const GENERATION: &str = "review-v1:fixture-head:sha256:fixture";

fn held_preflight_entries(profile: &Profile) -> Vec<LedgerEntry> {
    let mut preflight = LedgerEntry::new("test", profile, "auto", "review", "#1525", None, None);
    preflight.timestamp = "2026-11-06T00:00:00Z".into();
    preflight.work_id = Some("#1525".into());
    preflight.branch = Some("gah/1525".into());
    preflight.review_source_sha = Some("fixture-head".into());
    preflight.review_contract_version = Some(ledger::CURRENT_REVIEW_CONTRACT_VERSION);
    preflight.review_generation = Some(GENERATION.into());
    preflight.usage.usage_unknown_reason = Some(ledger::UsageUnknownReason::NoAttemptStarted);
    let mut refusal = preflight.clone();
    refusal.timestamp = "2026-11-06T00:00:01Z".into();
    refusal.validation_result = Some("review_budget_exhausted".into());
    refusal.human_required = true;
    refusal.human_required_reason_code = Some("retry_budget_exhausted".into());
    vec![preflight, refusal]
}

fn projected(cfg: &GahConfig, entries: &[LedgerEntry]) -> Vec<Blocker> {
    let mut mr = test_mr(Some("#1525"), "gah/1525");
    mr.review_generation = Some(GENERATION.into());
    let mut blockers = Vec::new();
    project_effective_mr_gates(
        cfg,
        "test",
        &cfg.profiles["test"],
        entries,
        &[mr],
        &mut blockers,
    );
    blockers
}

#[test]
fn incorrect_review_budget_hold_recovers_without_resetting_history() {
    let tmp = TempDir::new().unwrap();
    let cfg = make_test_cfg(&tmp);
    let profile = &cfg.profiles["test"];
    let entries = held_preflight_entries(profile);
    let gate =
        ledger::effective_human_gate_from_entries(&entries, "test", &profile.repo_id, "#1525")
            .unwrap();
    assert!(!execution_gate_still_required(
        &cfg, "test", profile, &entries, "#1525", &gate
    ));
    assert!(projected(&cfg, &entries).is_empty());
    assert!(entries[1].human_required, "only the projection changes");
}

#[test]
fn spent_paid_review_budget_and_unknown_attempts_remain_held() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = make_test_cfg(&tmp);
    cfg.defaults.routing.max_review_cycles_per_ticket = Some(5);
    cfg.defaults.routing.max_paid_reviews_per_ticket = Some(1);
    let profile = &cfg.profiles["test"];
    for known_paid in [true, false] {
        let mut entries = held_preflight_entries(profile);
        if known_paid {
            entries[0].validation_result = Some("NEEDS_FIX".into());
            entries[0].attempts_started = Some(1);
            entries[0].usage.usage_classification = Some("api_key_backed".into());
        } else {
            entries[0].attempts_started = None;
            entries[0].usage.usage_unknown_reason = None;
            cfg.defaults.routing.max_review_cycles_per_ticket = Some(1);
        }
        assert_eq!(projected(&cfg, &entries).len(), 1);
    }
}

#[test]
fn other_retry_holds_and_zero_review_limits_cannot_be_released() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = make_test_cfg(&tmp);
    let entries = held_preflight_entries(&cfg.profiles["test"]);
    cfg.defaults.routing.max_review_cycles_per_ticket = Some(0);
    assert_eq!(projected(&cfg, &entries).len(), 1);
    cfg.defaults.routing.max_review_cycles_per_ticket = Some(3);
    cfg.defaults.routing.max_paid_reviews_per_ticket = Some(0);
    assert_eq!(projected(&cfg, &entries).len(), 1);
    cfg.defaults.routing.max_paid_reviews_per_ticket = Some(3);
    let mut other_hold = entries;
    other_hold[1].validation_result = None;
    assert_eq!(projected(&cfg, &other_hold).len(), 1);
}

#[test]
fn another_work_item_profile_or_repository_cannot_reset_a_spent_review_budget() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = make_test_cfg(&tmp);
    cfg.defaults.routing.max_review_cycles_per_ticket = Some(1);
    for scope in ["work", "profile", "repo"] {
        let mut entries = held_preflight_entries(&cfg.profiles["test"]);
        entries[0].validation_result = Some("NEEDS_FIX".into());
        entries[0].attempts_started = Some(1);
        let mut other_reset = entries[0].clone();
        other_reset.mode = "clear_attempts".into();
        match scope {
            "work" => other_reset.work_id = Some("#999".into()),
            "profile" => other_reset.profile = "other".into(),
            "repo" => other_reset.repo_id = "other/repo".into(),
            _ => unreachable!(),
        }
        entries.push(other_reset);
        assert_eq!(projected(&cfg, &entries).len(), 1, "scope={scope}");
    }
}
