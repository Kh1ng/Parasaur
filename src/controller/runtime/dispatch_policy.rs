use super::NextAction;

/// Existing-branch repairs are intentionally launched against the MR's
/// current state, which may already be red for the exact defect the repair is
/// expected to fix. Let that unknown-red result reach the repair backend while
/// preserving the dispatch layer's unconditional stop for harness and
/// environment failures.
///
/// All other action types retain the direct-dispatch default: an unclassified
/// red baseline fails closed unless an operator explicitly overrides it.
pub(super) fn allow_unknown_red_baseline(action: &NextAction) -> bool {
    matches!(action, NextAction::FixMr { .. })
}

/// Persist a provider-only action's refusal using the same hold as dispatches.
pub(super) fn record_policy_refusal(
    cfg: &crate::config::GahConfig,
    profile_name: &str,
    action: &NextAction,
    run_id: Option<&str>,
    error: &anyhow::Error,
) -> anyhow::Result<String> {
    let (branch, mr_url) = match action {
        NextAction::MergeMr { branch, mr_url, .. }
        | NextAction::MarkReadyForReview { branch, mr_url, .. } => (branch, mr_url),
        _ => unreachable!("provider-only policy refusal received a dispatch action"),
    };
    let profile = crate::config::get_profile(cfg, profile_name)?;
    let mut entry = crate::ledger::LedgerEntry::new(
        profile_name,
        profile,
        "none",
        if matches!(action, NextAction::MergeMr { .. }) {
            "merge"
        } else {
            action.kind()
        },
        branch,
        run_id.map(str::to_string),
        None,
    );
    entry.work_id = Some(action.work_id().unwrap_or(branch).to_string());
    entry.branch = Some(branch.clone());
    entry.mr_url = mr_url.clone();
    crate::policy::stamp_refusal(&mut entry, error);
    crate::ledger::append_human_gate_if_transition(cfg, &entry)?;
    let outcome = format!("Blocked {} for branch '{branch}': {error:#}", action.kind());
    crate::events::record_dispatch_error(
        cfg,
        crate::events::EventType::DispatchFinished,
        Some(profile_name),
        entry.work_id.as_deref(),
        run_id,
        &outcome,
        Some("policy_approval"),
        error,
    )?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_fix_mr_allows_an_unknown_red_baseline_automatically() {
        assert!(allow_unknown_red_baseline(&NextAction::FixMr {
            work_id: Some("#624".into()),
            branch: "gah/repair".into(),
            mr_url: Some("https://example.test/pull/624".into()),
            review_generation: Some("review-v1:source:sha256:metadata".into()),
            reason: "review requested changes".into(),
        }));
        assert!(!allow_unknown_red_baseline(&NextAction::DispatchTicket {
            ticket_path: "#719".into(),
            work_id: Some("#719".into()),
            recommended_backend: None,
            recommended_model: None,
            reason: "new work".into(),
        }));
        assert!(!allow_unknown_red_baseline(&NextAction::Retry {
            work_id: "#719".into(),
            ticket_path: "#719".into(),
            reason: "retry".into(),
        }));
        assert!(!allow_unknown_red_baseline(&NextAction::Escalate {
            work_id: "#719".into(),
            ticket_path: "#719".into(),
            reason: "escalate".into(),
        }));
    }
}
