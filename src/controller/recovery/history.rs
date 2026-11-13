//! Journal queries for controller decisions. Keep the evidence each decision
//! actually consumes; the append-only journal remains the audit source.

use super::{timestamp_at_or_before, HumanRequiredReason, NextAction, STUCK_LOOP_THRESHOLD};
use crate::config::GahConfig;
use crate::events::ControllerEvent;
use anyhow::Result;
use std::collections::HashMap;

/// Newest capacity outcome per canonical work item and human report per exact
/// identity, including profile-wide reports. Append order, not timestamps,
/// determines which report supersedes an earlier one.
pub(in crate::controller) fn read_recovery_reports(
    cfg: &GahConfig,
    profile: &str,
) -> Result<Vec<ControllerEvent>> {
    let mut reports = HashMap::new();
    let mut position = 0usize;
    crate::events::visit_events(cfg, |event| {
        position += 1;
        if event.profile.as_deref() != Some(profile) {
            return;
        }
        let identity = match event.event_type.as_str() {
            "dispatch_finished" => event
                .work_id
                .as_deref()
                .map(crate::work_claim::normalize_work_identity),
            "human_required" => event.work_id.clone(),
            _ => return,
        };
        reports.insert((event.event_type.clone(), identity), (position, event));
    })?;
    let mut reports: Vec<_> = reports.into_values().collect();
    reports.sort_unstable_by_key(|(position, _)| *position);
    Ok(reports.into_iter().map(|(_, event)| event).collect())
}

/// Fold the selected action's decision suffix and route-approval suffix while
/// validating the entire journal. Memory is independent of journal length.
pub(in crate::controller) fn read_stuck_loop(
    cfg: &GahConfig,
    profile: &str,
    action: &NextAction,
    reset_after: Option<&str>,
    review_generation: Option<&str>,
) -> Result<Option<String>> {
    let work_id = action.work_id();
    let fingerprint = format!("{}:", action.kind());
    let generation = review_generation.map(|generation| format!("review_generation={generation}"));
    let mut decisions = 0usize;
    let mut approvals = 0usize;
    crate::events::visit_events(cfg, |event| {
        if work_id.is_none()
            || event.profile.as_deref() != Some(profile)
            || event.work_id.as_deref() != work_id
        {
            return;
        }
        if reset_after.is_some_and(|reset| timestamp_at_or_before(&event.timestamp, reset))
            || event.review_contract_version.unwrap_or(0)
                < crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION
        {
            decisions = 0;
            approvals = 0;
            return;
        }
        if event.event_type == "work_deferred"
            || (event.event_type == "dispatch_finished"
                && event.details.contains(": deferred_capacity:"))
        {
            decisions = 0;
        }
        if event.event_type == "action_decided"
            && generation.as_deref().is_none_or(|generation| {
                event
                    .details
                    .split_whitespace()
                    .any(|part| part == generation)
            })
        {
            decisions = if event.details.starts_with(&fingerprint) {
                (decisions + 1).min(STUCK_LOOP_THRESHOLD)
            } else {
                0
            };
        }
        if event.event_type == "dispatch_finished" {
            approvals = if event.reason_code.as_deref()
                == Some(HumanRequiredReason::PolicyApproval.as_str())
                || event.details.contains("operator approval required")
                || event.details.contains("paid route approval required")
            {
                (approvals + 1).min(STUCK_LOOP_THRESHOLD)
            } else {
                0
            };
        }
    })?;
    let Some(work_id) = work_id else {
        return Ok(None);
    };
    if decisions >= STUCK_LOOP_THRESHOLD {
        return Ok(Some(format!("stuck-loop detected: '{}' selected {} times in a row for {} with no intervening state change", action.kind(), STUCK_LOOP_THRESHOLD, work_id)));
    }
    if approvals >= STUCK_LOOP_THRESHOLD
        && matches!(
            action,
            NextAction::Retry { .. }
                | NextAction::Escalate { .. }
                | NextAction::DispatchTicket { .. }
        )
    {
        return Ok(Some(format!("stuck-loop detected: route approval required {} times in a row for {} with no intervening state change", STUCK_LOOP_THRESHOLD, work_id)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn event(kind: &str, details: &str) -> ControllerEvent {
        ControllerEvent {
            timestamp: "2026-11-05T20:00:00Z".into(),
            event_type: kind.into(),
            details: details.into(),
            profile: Some("test".into()),
            work_id: Some("#1".into()),
            run_id: Some("run-1".into()),
            review_contract_version: Some(crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION),
            reason_code: None,
            remediation_plan: None,
            skipped: Vec::new(),
            central_claim: None,
        }
    }

    fn write_events(cfg: &GahConfig, events: &[ControllerEvent]) {
        let mut file = std::fs::File::create(cfg.defaults.events_path()).unwrap();
        for event in events {
            serde_json::to_writer(&mut file, event).unwrap();
            writeln!(file).unwrap();
        }
    }

    #[test]
    fn streamed_decisions_match_reverse_history_across_boundaries_and_generations() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = GahConfig {
            context: Default::default(),
            defaults: crate::config::Defaults {
                artifact_root: tmp.path().to_string_lossy().into_owned(),
                ..Default::default()
            },
            profiles: HashMap::new(),
        };
        let actions = [
            NextAction::FixMr {
                work_id: Some("#1".into()),
                branch: "branch".into(),
                mr_url: None,
                review_generation: None,
                reason: "test".into(),
            },
            NextAction::DispatchTicket {
                work_id: Some("#1".into()),
                ticket_path: "ticket".into(),
                recommended_backend: None,
                recommended_model: None,
                reason: "test".into(),
            },
        ];
        let mut random = 19u64;
        for case in 0..64 {
            let mut events = Vec::new();
            for index in 0..48 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                let choice = if case < 4 { case } else { (random >> 32) % 14 };
                let mut next = match choice {
                    0 => event("action_decided", "fix_mr: test review_generation=g1"),
                    1 => event(
                        "action_decided",
                        "dispatch_ticket: test review_generation=g1",
                    ),
                    2 => event("dispatch_finished", "operator approval required"),
                    3 => event("dispatch_finished", "paid route approval required"),
                    4 => event("work_deferred", "attached branch"),
                    5 => event("dispatch_finished", "fix_mr: deferred_capacity: waiting"),
                    6 => event("action_decided", "fix_mr: test review_generation=g2"),
                    7 => {
                        let mut e = event("context_built", "legacy boundary");
                        e.review_contract_version = None;
                        e
                    }
                    8 => {
                        let mut e = event("action_decided", "fix_mr: other profile");
                        e.profile = Some("other".into());
                        e
                    }
                    9 => {
                        let mut e = event("dispatch_finished", "paid route approval required");
                        e.work_id = Some("#2".into());
                        e
                    }
                    10 => {
                        let mut e = event("human_required", "profile report");
                        e.work_id = None;
                        e
                    }
                    11 => event("duplicate_guard_triggered", "duplicate"),
                    12 => event("dispatch_started", "started"),
                    _ => event("dispatch_finished", "complete"),
                };
                if case >= 4 && index % 11 == 0 {
                    next.timestamp = "2026-11-05T19:00:00Z".into();
                }
                events.push(next);
            }
            write_events(&cfg, &events);
            for action in &actions {
                for reset in [
                    None,
                    Some("2026-11-05T19:30:00Z"),
                    Some("2026-11-05T21:00:00Z"),
                ] {
                    for generation in [None, Some("g1"), Some("g2")] {
                        assert_eq!(
                            read_stuck_loop(&cfg, "test", action, reset, generation).unwrap(),
                            super::super::detect_stuck_loop(
                                &events, "test", action, reset, generation
                            ),
                            "case {case}, reset {reset:?}, generation {generation:?}"
                        );
                    }
                }
            }
            assert_eq!(
                crate::events::read_orphaned_dispatch_runs(&cfg, "test").unwrap(),
                crate::events::orphaned_dispatch_runs(&events, "test")
            );
            let compact = read_recovery_reports(&cfg, "test").unwrap();
            assert!(compact.len() <= 4, "history grew with repeated events");
            for work_id in [None, Some("#1"), Some("#2")] {
                for kind in ["human_required", "dispatch_finished"] {
                    let last = |records: &[ControllerEvent]| {
                        records
                            .iter()
                            .rev()
                            .find(|e| {
                                e.profile.as_deref() == Some("test")
                                    && e.event_type == kind
                                    && e.work_id.as_deref() == work_id
                            })
                            .map(|e| serde_json::to_value(e).unwrap())
                    };
                    assert_eq!(last(&compact), last(&events));
                }
            }
        }
        std::fs::OpenOptions::new()
            .append(true)
            .open(cfg.defaults.events_path())
            .unwrap()
            .write_all(b"{corrupt\n")
            .unwrap();
        assert!(read_stuck_loop(&cfg, "test", &actions[0], None, None).is_err());
        assert!(read_recovery_reports(&cfg, "test").is_err());
        assert!(crate::events::read_orphaned_dispatch_runs(&cfg, "test").is_err());
    }
}
