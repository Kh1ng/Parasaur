use super::*;
use crate::ledger::{test_util::profile, AttemptRecord, LedgerUsage};

fn at(raw: &str) -> OffsetDateTime {
    OffsetDateTime::parse(raw, &Rfc3339).unwrap()
}
fn budget() -> SpendBudget {
    SpendBudget {
        daily_usd: Some(1.0),
        ..Default::default()
    }
}
fn entry(cost: Option<f64>) -> LedgerEntry {
    let mut entry = LedgerEntry::new("test", &profile(), "codex", "fix", "task", None, None);
    entry.timestamp = "2030-01-31T12:00:00Z".into();
    entry.usage.actual_cost_usd = cost;
    entry
}
fn status(budget: &SpendBudget, entries: &[LedgerEntry]) -> SpendBudgetStatus {
    evaluate(Some(budget), "test", entries, at("2030-01-31T13:00:00Z")).unwrap()
}

#[test]
fn mirrored_cost_counts_once_and_modern_missing_attempts_stay_unknown() {
    let mut entry = entry(Some(99.0));
    entry.attempts = vec![
        AttemptRecord {
            usage: LedgerUsage {
                actual_cost_usd: Some(0.5),
                ..Default::default()
            },
            ..Default::default()
        },
        AttemptRecord {
            usage: LedgerUsage::default(),
            ..Default::default()
        },
    ];
    let report = status(&budget(), &[entry.clone()]);
    assert_eq!(report.windows[0].reported_usd, Some(0.5));
    assert_eq!(report.windows[0].measured_observations, 1);
    assert_eq!(report.windows[0].unmeasured_observations, 1);
    assert!(report.intake_held);
    // Historical attempts with no readings retain the legacy summary once.
    entry.attempts[0].usage = LedgerUsage::default();
    let report = status(&budget(), &[entry]);
    assert_eq!(report.windows[0].reported_usd, Some(99.0));
    assert_eq!(report.windows[0].measured_observations, 1);
    assert!(report.windows[0].exhausted);
}

#[test]
fn actual_and_estimated_are_explicit_independent_bases() {
    let mut entry = entry(None);
    entry.usage.estimated_cost_usd = Some(0.25);
    let report = status(&budget(), &[entry.clone()]);
    assert!(report.intake_held);
    assert_eq!(report.windows[0].reported_usd, None);
    let mut budget = budget();
    budget.cost_basis = SpendCostBasis::Estimated;
    let report = status(&budget, &[entry]);
    assert!(!report.intake_held);
    assert_eq!(report.windows[0].reported_usd, Some(0.25));
}

#[test]
fn utc_day_and_month_windows_roll_over_without_clearing_history() {
    let entries = [entry(Some(1.0))];
    let mut budget = budget();
    budget.monthly_usd = Some(2.0);
    assert!(status(&budget, &entries).intake_held);
    let report = evaluate(Some(&budget), "test", &entries, at("2030-02-01T00:00:00Z")).unwrap();
    assert!(!report.intake_held);
    assert!(report
        .windows
        .iter()
        .all(|window| window.reported_usd == Some(0.0)));
    let report = evaluate(Some(&budget), "test", &[], at("2030-12-31T23:59:59Z")).unwrap();
    assert_eq!(report.windows[1].resets_at, "2031-01-01T00:00:00Z");
    let mut offset = entry(Some(1.0));
    offset.timestamp = "2030-02-01T01:00:00+02:00".into();
    assert!(
        evaluate(Some(&budget), "test", &[offset], at("2030-01-31T23:30:00Z"))
            .unwrap()
            .intake_held
    );
}

#[test]
fn controls_other_profiles_and_proven_preflights_do_not_consume_spend() {
    let mut other = entry(Some(100.0));
    other.profile = "another".into();
    let mut control = entry(None);
    control.mode = "clear_attempts".into();
    let mut unstarted = entry(None);
    unstarted.attempts_started = Some(0);
    unstarted.attempts_completed = Some(0);
    unstarted.usage.usage_unknown_reason = Some(ledger::UsageUnknownReason::NoAttemptStarted);
    let report = status(&budget(), &[other, control, unstarted]);
    assert!(!report.intake_held);
    assert_eq!(report.windows[0].reported_usd, Some(0.0));
    assert!(evaluate(None, "test", &[entry(None)], at("2030-01-31T13:00:00Z")).is_none());
}

#[test]
fn malformed_readings_and_exhausted_zero_budget_fail_closed() {
    for cost in [None, Some(-1.0), Some(f64::NAN), Some(f64::INFINITY)] {
        assert!(status(&budget(), &[entry(cost)]).intake_held);
    }
    let mut invalid = entry(Some(0.0));
    invalid.timestamp = "invalid".into();
    assert!(status(&budget(), &[invalid]).intake_held);
    let mut invalid = entry(Some(0.0));
    invalid.timestamp = "2030-01-31T15:00:00Z".into();
    assert!(status(&budget(), &[invalid]).intake_held);
    assert!(
        status(
            &SpendBudget {
                daily_usd: Some(0.0),
                ..Default::default()
            },
            &[]
        )
        .intake_held
    );
    assert!(
        status(
            &SpendBudget {
                daily_usd: Some(-1.0),
                ..Default::default()
            },
            &[]
        )
        .intake_held
    );
    let report = status(&budget(), &[entry(Some(f64::MAX)), entry(Some(f64::MAX))]);
    assert!(report.intake_held);
    assert_eq!(report.windows[0].reported_usd, None);
}

#[test]
fn controller_pauses_then_resumes_without_a_work_item_human_gate() {
    let held = status(&budget(), &[entry(Some(1.0))]);
    let action = held.wait_action().unwrap();
    assert_eq!(action.kind(), "wait_until");
    assert_eq!(action.work_id(), None);
    assert!(action.reason().starts_with("spend_budget:"));
    let resumed = evaluate(
        Some(&budget()),
        "test",
        &[entry(Some(1.0))],
        at("2030-02-01T00:00:00Z"),
    )
    .unwrap();
    assert!(resumed.wait_action().is_none());
}

#[test]
fn decimal_threshold_rounding_never_admits_an_extra_dispatch() {
    let entries = (0..10).map(|_| entry(Some(0.1))).collect::<Vec<_>>();
    assert!(status(&budget(), &entries).windows[0].exhausted);
}

#[test]
fn an_earlier_day_reset_does_not_release_an_exhausted_month() {
    let mut old = entry(Some(2.0));
    old.timestamp = "2030-01-30T12:00:00Z".into();
    let budget = SpendBudget {
        daily_usd: Some(1.0),
        monthly_usd: Some(2.0),
        ..Default::default()
    };
    let report = status(&budget, &[old]);
    assert!(!report.windows[0].exhausted);
    assert!(report.windows[1].exhausted);
    assert!(report.intake_held);
    let report = evaluate(
        Some(&budget),
        "test",
        &[entry(Some(1.0))],
        at("2030-01-31T23:59:59Z"),
    )
    .unwrap();
    assert_eq!(report.retry_at, "2030-02-01T00:00:00Z");
}

#[test]
fn failed_pause_hook_is_recorded_and_does_not_fail_intake_observation() {
    let (_tmp, mut cfg) = crate::ledger::test_util::test_config();
    let mut profile = profile();
    profile.notify_command = Some("exit 42".into());
    cfg.profiles.insert("test".into(), profile);
    let action = status(&budget(), &[entry(Some(1.0))])
        .wait_action()
        .unwrap();
    record_action(&cfg, "test", &action).unwrap();
    assert!(crate::events::read_events(&cfg)
        .unwrap()
        .iter()
        .any(|event| event.event_type == "notification_delivery_failed"));
    assert_eq!(action.kind(), "wait_until");
}

#[test]
fn late_budget_refusal_is_a_cli_error_but_a_controller_pause() {
    let (_tmp, mut cfg) = crate::ledger::test_util::test_config();
    let mut profile = profile();
    profile.routing.spend_budget = Some(SpendBudget {
        daily_usd: Some(0.0),
        ..Default::default()
    });
    cfg.profiles.insert("test".into(), profile);
    let action = crate::controller::NextAction::DispatchTicket {
        ticket_path: "unused".into(),
        work_id: Some("TICKET-001".into()),
        recommended_backend: None,
        recommended_model: None,
        reason: "fixture".into(),
    };
    let error =
        crate::controller::execute_action(&cfg, "test", &action, None, true, None).unwrap_err();
    assert!(error.downcast_ref::<SpendBudgetHeld>().is_some());
    let admission = crate::controller::RouteNodeAdmission::single_worker(
        action.clone(),
        cfg.defaults.node_capacity,
    );
    let outcome =
        crate::controller::execute_action(&cfg, "test", &action, None, true, Some(admission))
            .unwrap();
    assert!(outcome.starts_with("Deferred "));
    assert!(outcome.contains("no backend launched"));
    assert!(ledger::read_entries(&cfg).unwrap().is_empty());
}

#[test]
fn configuration_inherits_as_a_whole_and_empty_profile_policy_disables_it() {
    let mut defaults = crate::config::Defaults::default();
    defaults.routing.spend_budget = Some(budget());
    let mut profile = profile();
    assert_eq!(
        profile.effective_routing(&defaults).spend_budget,
        Some(budget())
    );
    profile.routing.spend_budget = Some(SpendBudget::default());
    assert!(!profile
        .effective_routing(&defaults)
        .spend_budget
        .unwrap()
        .is_enforced());
    for value in ["daily_usd = -1.0", "monthly_usd = nan", "daily_usd = inf"] {
        assert!(toml::from_str::<SpendBudget>(value)
            .unwrap()
            .validate()
            .is_err());
    }
    assert!(toml::from_str::<SpendBudget>("daily_us = 1.0").is_err());
}

#[test]
fn notice_is_once_per_transition_and_never_wakes_an_agent() {
    let (_tmp, mut cfg) = crate::ledger::test_util::test_config();
    cfg.profiles.insert("test".into(), profile());
    let held = status(&budget(), &[entry(Some(1.0))])
        .wait_action()
        .unwrap();
    record_action(&cfg, "test", &held).unwrap();
    record_action(&cfg, "test", &held).unwrap();
    assert_eq!(
        crate::events::read_events(&cfg)
            .unwrap()
            .iter()
            .filter(|event| event.event_type == "spend_budget_held")
            .count(),
        1
    );
    record_action(
        &cfg,
        "test",
        &crate::controller::NextAction::NoOp {
            reason: "resumed".into(),
        },
    )
    .unwrap();
    record_action(&cfg, "test", &held).unwrap();
    assert_eq!(
        crate::events::read_events(&cfg)
            .unwrap()
            .iter()
            .filter(|event| event.event_type == "spend_budget_held")
            .count(),
        2
    );
    assert!(crate::notifications::format_wake_instruction(
        &crate::notifications::NotifyEvent::SpendBudgetHeld {
            profile: "test",
            reason: "paused"
        },
        crate::config::WakeAutonomy::Full
    )
    .is_none());
}
