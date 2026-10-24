use crate::config::GahConfig;
use crate::ledger::LedgerEntry;

pub(super) fn is_policy_approval_gate(entry: &LedgerEntry) -> bool {
    entry.human_required
        && entry.human_required_reason_code.as_deref()
            == Some(crate::controller::HumanRequiredReason::PolicyApproval.as_str())
        && entry.failure_class.as_deref()
            == Some(crate::ledger::FailureClass::HumanBlocked.as_str())
}

pub(super) fn append_ledger_entry(
    cfg: &GahConfig,
    ledger: &LedgerEntry,
    policy_approval_gate: bool,
) -> anyhow::Result<bool> {
    if policy_approval_gate {
        let appended = crate::ledger::append_human_gate_if_transition(cfg, ledger)?;
        if appended {
            return Ok(true);
        }
    }
    crate::ledger::append(cfg, ledger).map(|_| false)
}

pub(super) fn should_notify_dispatch_failure(error: &anyhow::Error) -> bool {
    if super::review_budget_exhausted_error(error).is_some()
        || super::capacity_deferred_error(error)
        || super::issue_claim_lost(error).is_some()
    {
        return false;
    }
    // Approval skips have their own candidate-scoped notice. Ordinary quota
    // and authentication backpressure needs no terminal failure alert.
    !error
        .downcast_ref::<crate::routing::RouteError>()
        .is_some_and(|route| {
            let skipped = match route {
                crate::routing::RouteError::ApprovalRequired { .. } => return true,
                crate::routing::RouteError::NoEligibleBackend { skipped, .. } => skipped,
            };
            skipped
                .iter()
                .any(|skip| skip.reason == "operator_approval_required")
                || (!skipped.is_empty()
                    && skipped.iter().all(|skip| {
                        skip.reason.contains("quota_exhausted")
                            || skip.reason.contains("authentication_error")
                    }))
        })
}

/// Attribute terminal errors while keeping initial node rejection out of
/// execution attempt counts. Real prior attempts retain their attribution.
pub(super) fn attribute_terminal_failure(ledger: &mut LedgerEntry, err: &anyhow::Error) {
    if super::node_capacity_deferred_error(err)
        && super::post_attempt_capacity_deferral(err).is_none()
    {
        ledger.validation_result = Some("deferred_capacity".into());
        ledger.attempts_started = Some(0);
    }
    if crate::policy::policy_blocked(err).is_some() {
        crate::policy::stamp_refusal(ledger, err);
    }
    // Every terminal dispatch error must be attributable. More precise
    // workflow boundaries set their own class/stage; this fallback marks
    // uncaught orchestration/plumbing errors as harness-owned rather than
    // sending an operationally useless class=unknown/stage=unknown alert.
    ensure_terminal_failure_attribution(&mut ledger.failure_class, &mut ledger.failure_stage);
    ledger.error_summary = Some(super::error::summarize_error(err));
}

pub(super) fn ensure_terminal_failure_attribution(
    failure_class: &mut Option<String>,
    failure_stage: &mut Option<String>,
) {
    failure_class.get_or_insert_with(|| {
        crate::ledger::FailureClass::HarnessError
            .as_str()
            .to_string()
    });
    failure_stage.get_or_insert_with(|| crate::ledger::FailureStage::Dispatch.as_str().to_string());
}
