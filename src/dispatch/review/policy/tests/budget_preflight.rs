use super::*;

fn preflight_review(prof: &Profile) -> LedgerEntry {
    let mut entry = review_ledger_entry("test", prof, "gah/42", "NEEDS_FIX", "high");
    entry.work_id = Some("#42".into());
    entry.validation_result = None;
    entry.usage.usage_unknown_reason = Some(crate::ledger::UsageUnknownReason::NoAttemptStarted);
    entry.set_failure(
        crate::ledger::FailureClass::HarnessError,
        crate::ledger::FailureStage::Dispatch,
    );
    entry
}

#[test]
fn review_preflight_failures_do_not_spend_cycles_or_paid_reviews() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = gah_config_with_ledger(
        tmp.path(),
        RoutingPolicy {
            max_review_cycles_per_ticket: Some(1),
            max_paid_reviews_per_ticket: Some(1),
            ..Default::default()
        },
    );
    let prof = profile(tmp.path());
    for _ in 0..5 {
        let mut entry = preflight_review(&prof);
        entry.usage.usage_classification = Some("api_key_backed".into());
        crate::ledger::append(&cfg, &entry).unwrap();
    }
    assert!(
        check_review_budget(&cfg, &prof, "test", Some("#42"), &paid_route_decision())
            .unwrap()
            .is_none()
    );
    let mut actual = review_ledger_entry("test", &prof, "gah/42", "NEEDS_FIX", "high");
    actual.work_id = Some("#42".into());
    actual.attempts_started = Some(1);
    actual.attempts_completed = Some(1);
    actual.usage.usage_classification = Some("api_key_backed".into());
    crate::ledger::append(&cfg, &actual).unwrap();
    assert!(
        check_review_budget(&cfg, &prof, "test", Some("#42"), &paid_route_decision())
            .unwrap()
            .is_some(),
        "a real review still spends the cap"
    );
}

#[test]
fn missing_or_conflicting_execution_evidence_still_spends_a_review_cycle() {
    type EvidenceMutation = fn(&mut LedgerEntry);
    let cases: &[(&str, EvidenceMutation)] = &[
        ("started", |entry| entry.attempts_started = Some(1)),
        ("unknown started", |entry| entry.attempts_started = None),
        ("completed", |entry| entry.attempts_completed = Some(1)),
        ("unknown completed", |entry| entry.attempts_completed = None),
        ("exit code", |entry| entry.backend_exit_code = Some(0)),
        ("attempt", |entry| {
            entry.attempts.push(crate::ledger::AttemptRecord::default())
        }),
        ("route attempt", |entry| {
            entry
                .attempt_routing
                .push(crate::ledger::AttemptRoutingRecord::default())
        }),
        ("verdict", |entry| {
            entry.review_verdict = Some("APPROVE".into())
        }),
        ("validation", |entry| {
            entry.validation_result = Some("NEEDS_FIX".into())
        }),
        ("tokens including zero", |entry| {
            entry.usage.total_tokens = Some(0)
        }),
        ("cost including zero", |entry| {
            entry.usage.actual_cost_usd = Some(0.0)
        }),
        ("usage source", |entry| {
            entry.usage.usage_source = Some("fixture".into())
        }),
        ("missing artifact", |entry| {
            entry.usage.usage_unknown_reason =
                Some(crate::ledger::UsageUnknownReason::UsageArtifactMissing)
        }),
        ("legacy unknown", |entry| {
            entry.schema_version = 9;
            entry.usage.usage_unknown_reason = None;
        }),
    ];
    for (name, mutate) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = gah_config_with_ledger(
            tmp.path(),
            RoutingPolicy {
                max_review_cycles_per_ticket: Some(1),
                ..Default::default()
            },
        );
        let prof = profile(tmp.path());
        let mut entry = preflight_review(&prof);
        mutate(&mut entry);
        crate::ledger::append(&cfg, &entry).unwrap();
        assert!(
            check_review_budget(
                &cfg,
                &prof,
                "test",
                Some("#42"),
                &route_decision("vibe", Some("reviewer"), false)
            )
            .unwrap()
            .is_some(),
            "{name} cannot establish an unstarted review"
        );
    }
}
