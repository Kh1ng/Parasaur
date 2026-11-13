use super::Blocker;
use crate::config::{GahConfig, Profile};
use crate::controller::HumanRequiredReason;
use crate::job_kind::JobKind;
use crate::ledger::{self, LedgerEntry};
use crate::sync;

pub(super) fn execution_gate_still_required(
    cfg: &GahConfig,
    profile_name: &str,
    profile: &Profile,
    entries: &[LedgerEntry],
    work_id: &str,
    gate: &ledger::EffectiveHumanGate,
) -> bool {
    if crate::dispatch::review_budget_gate_is_resolved(
        cfg,
        profile_name,
        profile,
        entries,
        work_id,
        gate,
    ) {
        return false;
    }
    if gate.reason_code.as_deref() != Some(HumanRequiredReason::PolicyApproval.as_str()) {
        return true;
    }
    // Backend recovery cannot authorize a repository mutation refused by its policy.
    if gate.dispatch_reason.as_deref() == Some("repo_policy_blocked") {
        return true;
    }
    // TICKET-711: only a review-derived policy_approval gate (raised during
    // review escalation or its post-review repair) is superseded by a stale
    // contract version. A genuine non-review paid-route hold (a regular
    // fix/improve dispatch still awaiting `gah route-approval grant`) must
    // not be silently released just because it predates the contract bump.
    let review_derived = JobKind::parse(&gate.mode) == Ok(JobKind::Review)
        || gate.dispatch_reason.as_deref() == Some("post_review_repair");
    if review_derived
        && gate.review_contract_version.unwrap_or(0)
            < crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION
    {
        return false;
    }
    let mode: &'static str = match JobKind::parse(&gate.mode) {
        Ok(JobKind::Review) => "review",
        Ok(JobKind::Pm) => "pm",
        Ok(JobKind::Experiment) => "experiment",
        _ => "fix",
    };
    let mut current = LedgerEntry::new(profile_name, profile, "auto", mode, work_id, None, None);
    current.work_id = Some(work_id.to_string());
    let runtime = crate::dispatch::routing_runtime_state_from_entries(entries, &current);
    let route_is_eligible = |backend: &str, model: Option<&str>| {
        // Review escalation is ordered and one-shot per backend/model for an
        // unchanged source. A recovered reviewer that already produced the
        // verdict which led to this paid-route gate is not a real escape
        // route: dispatch would only write skipped_duplicate_review and ask
        // for the same paid approval again on the next tick.
        if mode == "review"
            && review_route_was_already_used(
                entries,
                profile_name,
                &profile.repo_id,
                work_id,
                backend,
                model,
                gate.review_generation.as_deref(),
            )
        {
            return false;
        }
        let request = crate::routing::RouteRequest {
            mode,
            requested_backend: backend,
            requested_instance: None,
            requested_model: model,
            recommended_backend: None,
            recommended_model: None,
            session_id: None,
            usage_summary: None,
            last_failure_class: None,
            exact_route_required: false,
        };
        crate::routing::decide_with_state(&cfg.defaults, profile, request, &runtime)
            .ok()
            .is_some_and(|decision| {
                mode != "review"
                    || !review_route_was_already_used(
                        entries,
                        profile_name,
                        &profile.repo_id,
                        work_id,
                        &decision.effective_backend,
                        decision.effective_model.as_deref(),
                        gate.review_generation.as_deref(),
                    )
            })
    };

    // New route failures preserve the exact candidates that were considered.
    // Re-probe those identities explicitly so implementation task rules are
    // honored even though status does not re-fetch the source issue's task
    // metadata. Attempted capability failures remain excluded by `runtime`.
    if let Some(diagnostics) = gate
        .routing_diagnostics
        .as_ref()
        .filter(|diagnostics| !diagnostics.candidates.is_empty())
    {
        return !diagnostics
            .candidates
            .iter()
            .any(|candidate| route_is_eligible(&candidate.backend, candidate.model.as_deref()));
    }

    // Legacy records predate structured route-error diagnostics. Reviews and
    // PM tasks have one mode-level candidate list, so an auto-route probe is
    // exact for those modes. Implementation records fail closed because a
    // task-specific rule may differ from the generic candidate list.
    if matches!(mode, "review" | "pm") {
        return !route_is_eligible("auto", None);
    }
    true
}

fn review_route_was_already_used(
    entries: &[LedgerEntry],
    profile_name: &str,
    repo_id: &str,
    work_id: &str,
    backend: &str,
    model: Option<&str>,
    review_generation: Option<&str>,
) -> bool {
    let aliases = ledger::work_id_aliases(work_id);
    let reset_index = entries.iter().rposition(|entry| {
        entry.profile == profile_name
            && entry.repo_id == repo_id
            && entry.mode == "clear_attempts"
            && entry
                .work_id
                .as_deref()
                .is_some_and(|id| aliases.iter().any(|alias| alias == id))
    });
    let active = reset_index.map_or(entries, |index| &entries[index + 1..]);
    active.iter().any(|entry| {
        entry.profile == profile_name
            && entry.repo_id == repo_id
            && JobKind::parse(&entry.mode) == Ok(JobKind::Review)
            && entry.review_contract_version == Some(ledger::CURRENT_REVIEW_CONTRACT_VERSION)
            && entry.review_generation.as_deref() == review_generation
            && entry
                .work_id
                .as_deref()
                .is_some_and(|id| aliases.iter().any(|alias| alias == id))
            && entry.effective_backend == backend
            && entry.effective_model.as_deref() == model
            && entry.review_verdict.is_some()
            && entry.failure_class.is_none()
    })
}

fn mr_work_id_from_ledger<'a>(
    mr: &'a sync::SyncMrJson,
    entries: &'a [LedgerEntry],
    profile_name: &str,
    repo_id: &str,
) -> Option<&'a str> {
    mr.work_id.as_deref().or_else(|| {
        entries.iter().rev().find_map(|entry| {
            (entry.profile == profile_name
                && entry.repo_id == repo_id
                && entry.work_id.is_some()
                && (entry.branch.as_deref() == Some(mr.branch.as_str())
                    || (mr.url.is_some() && entry.mr_url.as_deref() == mr.url.as_deref())))
            .then_some(entry.work_id.as_deref())
            .flatten()
        })
    })
}

pub(super) fn project_effective_mr_gates(
    cfg: &GahConfig,
    profile_name: &str,
    profile: &Profile,
    entries: &[LedgerEntry],
    merge_requests: &[sync::SyncMrJson],
    blocked_work_items: &mut Vec<Blocker>,
) {
    let index = ledger::index_entries_by_work_id(entries);
    for blocker in blocked_work_items.iter_mut().filter(|blocker| {
        blocker.reason_code.as_deref() == Some(HumanRequiredReason::RepeatedSetupFailure.as_str())
    }) {
        if let Some(work_id) = blocker.source_reference.as_deref() {
            blocker.message = crate::models::AvailableTicket::setup_failure_reason_for_ticket(
                work_id, profile, &index,
            );
        }
    }
    for mr in merge_requests {
        let Some(work_id) = mr_work_id_from_ledger(mr, entries, profile_name, &profile.repo_id)
        else {
            continue;
        };
        let Some(gate) = ledger::effective_human_gate_from_entries(
            entries,
            profile_name,
            &profile.repo_id,
            work_id,
        ) else {
            continue;
        };
        let review_derived = gate.dispatch_reason.as_deref() != Some("repo_policy_blocked")
            && (JobKind::parse(&gate.mode) == Ok(JobKind::Review)
                || (gate.reason_code.as_deref() == Some("stuck_loop_gate")
                    && gate.review_generation.is_some()));
        if review_derived
            && (gate.review_contract_version != Some(ledger::REVIEW_CONTRACT_VERSION)
                || gate.review_generation.as_deref() != mr.review_generation.as_deref())
        {
            continue;
        }
        if !execution_gate_still_required(cfg, profile_name, profile, entries, work_id, &gate) {
            continue;
        }
        let reason_code = gate.reason_code.clone();
        if let Some(blocker) = blocked_work_items.iter_mut().find(|blocker| {
            blocker.kind == "human_required" && blocker.source_reference.as_deref() == Some(work_id)
        }) {
            // Ticket discovery can project the same gate first, but it only
            // carries a generic message. Enrich that row with the exact MR
            // gate details so the dashboard and notifications explain what
            // is blocked and how to release it without adding a duplicate.
            blocker.reason = reason_code
                .clone()
                .or_else(|| gate.dispatch_reason.clone())
                .or_else(|| blocker.reason.clone());
            if let Some(message) = gate.message.clone() {
                blocker.message = Some(message);
            }
            blocker.reason_code = reason_code;
            continue;
        }
        blocked_work_items.push(Blocker {
            kind: "human_required".into(),
            reason: reason_code
                .clone()
                .or_else(|| gate.dispatch_reason.clone())
                .or(Some("ledger_human_required".into())),
            message: gate
                .message
                .clone()
                .or(Some("Ledger indicates human intervention required".into())),
            backend: None,
            model: None,
            quota_pool: None,
            until: None,
            source_reference: Some(work_id.to_string()),
            reason_code,
            remediation_plan: None,
        });
    }
}

#[cfg(test)]
mod tests;
