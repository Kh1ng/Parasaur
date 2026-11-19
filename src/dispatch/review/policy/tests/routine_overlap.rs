use super::*;

fn overlapping_profile(root: &std::path::Path, escalation_model: Option<&str>) -> Profile {
    let mut prof = profile(root);
    prof.routing.routine_reviewer = Some(CandidateConfig {
        backend: "opencode".into(),
        model: Some("routine-model".into()),
        ..Default::default()
    });
    prof.routing.escalatory_reviewers = vec![CandidateConfig {
        backend: "opencode".into(),
        model: escalation_model.map(str::to_string),
        ..Default::default()
    }];
    prof
}

#[test]
fn routine_identity_is_not_promoted_by_an_overlapping_escalation_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = gah_config(RoutingPolicy::default());
    let route = route_decision("opencode", Some("routine-model"), false);
    for model in [None, Some("routine-model")] {
        let prof = overlapping_profile(tmp.path(), model);
        assert_eq!(
            derive_reviewer_tier(&cfg, &prof, &route),
            ReviewerTier::Strong
        );
    }
    let prof = overlapping_profile(tmp.path(), None);
    assert_eq!(
        derive_reviewer_tier(
            &cfg,
            &prof,
            &route_decision("opencode", Some("different-model"), true)
        ),
        ReviewerTier::Escalatory
    );
    let mut prof = overlapping_profile(tmp.path(), Some("different-model"));
    prof.routing.routine_reviewer.as_mut().unwrap().model = None;
    assert_eq!(
        derive_reviewer_tier(
            &cfg,
            &prof,
            &route_decision("opencode", Some("different-model"), true)
        ),
        ReviewerTier::Escalatory
    );
}

#[test]
fn inherited_routine_identity_wins_over_a_profile_wide_escalation_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let mut prof = overlapping_profile(tmp.path(), None);
    let routine = prof.routing.routine_reviewer.take();
    let cfg = gah_config(RoutingPolicy {
        routine_reviewer: routine,
        ..Default::default()
    });
    assert_eq!(
        derive_reviewer_tier(
            &cfg,
            &prof,
            &route_decision("opencode", Some("routine-model"), false)
        ),
        ReviewerTier::Strong
    );
}

#[test]
fn overlapping_routine_cannot_bypass_the_review_cycle_cap_as_untried_escalation() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = gah_config_with_ledger(
        tmp.path(),
        RoutingPolicy {
            max_review_cycles_per_ticket: Some(1),
            ..Default::default()
        },
    );
    let prof = overlapping_profile(tmp.path(), None);
    let mut prior = review_ledger_entry("test", &prof, "gah/42", "NEEDS_FIX", "high");
    prior.work_id = Some("#42".into());
    crate::ledger::append(&cfg, &prior).unwrap();
    let block = check_review_budget(
        &cfg,
        &prof,
        "test",
        Some("#42"),
        &route_decision("opencode", Some("routine-model"), false),
    )
    .unwrap()
    .expect("a routine reviewer must honor the exhausted review-cycle cap");
    assert!(block.reason.contains("1/1 review cycles"));
}
