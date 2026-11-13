use super::{dispatch_policy, merge, pm, run_dispatch_and_record, RouteNodeAdmission};
use crate::controller::NextAction;
use anyhow::Result;

/// Select the bounded retry evidence needed by this action before handing it
/// to a worker. The full ledger snapshot need not survive action execution.
pub(crate) fn action_prior_context(
    cfg: &crate::config::GahConfig,
    profile_name: &str,
    action: &NextAction,
    ledger_entries: &[crate::ledger::LedgerEntry],
) -> Result<Option<String>> {
    match action {
        NextAction::Retry { work_id, .. } | NextAction::Escalate { work_id, .. } => {
            let profile = crate::config::get_profile(cfg, profile_name)?;
            Ok(crate::dispatch::prior_attempt_context(
                ledger_entries,
                profile_name,
                profile,
                work_id,
            ))
        }
        _ => Ok(None),
    }
}

/// Executes at most one action. `FixMr` reuses its branch (TICKET-118);
/// Retry/Escalate receive their bounded, already selected prior evidence.
pub(crate) fn execute_action(
    cfg: &crate::config::GahConfig,
    profile_name: &str,
    action: &NextAction,
    prior_attempt_context: Option<String>,
    skip_validation_gate: bool,
    route_admission: Option<RouteNodeAdmission>,
) -> Result<String> {
    let profile = crate::config::get_profile(cfg, profile_name)?;
    let base_args = || crate::dispatch::DispatchArgs {
        profile: profile_name.to_string(),
        mode: "fix".to_string(),
        backend: "auto".to_string(),
        backend_instance: None,
        target: String::new(),
        branch: None,
        mr: None,
        current_branch: false,
        dry_run: false,
        oh_profile: None,
        model: None,
        retries: 2,
        allow_draft_fail: false,
        prod: false,
        issue_intake_override: false,
        allow_unknown_red_baseline: dispatch_policy::allow_unknown_red_baseline(action),
        escalate: false,
        existing_branch: None,
        expected_review_generation: None,
        skip_validation_gate,
        dispatch_reason: None,
        prior_attempt_context: prior_attempt_context.clone(),
        work_id: action.work_id().map(str::to_string),
        run_id: Some(uuid::Uuid::new_v4().to_string()),
        route_admission: route_admission.clone(),
    };

    match action {
        NextAction::ReviewMr { branch, .. } => {
            let args = crate::dispatch::DispatchArgs {
                mode: "review".to_string(),
                branch: Some(branch.clone()),
                expected_review_generation: route_admission
                    .as_ref()
                    .and_then(|admission| admission.review_generation())
                    .map(str::to_string),
                dispatch_reason: Some("review".to_string()),
                ..base_args()
            };
            let deferred = match run_dispatch_and_record(cfg, "review", action.work_id(), &args) {
                // A durable work-item hold must not put unrelated work into
                // the daemon's infrastructure-error backoff. Direct dispatch
                // retains its typed error and terminal event.
                Err(error) if crate::dispatch::review_budget_exhausted_error(&error).is_some() => {
                    return Ok(format!("Blocked review: {error:#}"));
                }
                result => result?,
            };
            Ok(deferred.unwrap_or_else(|| format!("Dispatched review for branch '{branch}'")))
        }
        NextAction::MarkReadyForReview { branch, .. } => {
            if let Err(error) = crate::provider::mark_ready_for_review(profile, branch) {
                if crate::policy::policy_blocked(&error).is_some() {
                    return dispatch_policy::record_policy_refusal(
                        cfg,
                        profile_name,
                        action,
                        None,
                        &error,
                    );
                }
                return Err(error);
            }
            Ok(format!("Marked MR on branch '{branch}' ready for review"))
        }
        NextAction::FixMr {
            branch,
            review_generation,
            ..
        } => {
            let args = crate::dispatch::DispatchArgs {
                target: branch.clone(),
                existing_branch: Some(branch.clone()),
                expected_review_generation: review_generation.clone(),
                dispatch_reason: Some("post_review_repair".to_string()),
                ..base_args()
            };
            let deferred = run_dispatch_and_record(cfg, "fix_existing", action.work_id(), &args)?;
            Ok(
                deferred
                    .unwrap_or_else(|| format!("Dispatched fix for existing branch '{branch}'")),
            )
        }
        NextAction::MergeMr { .. } => merge::execute(cfg, profile_name, action),
        NextAction::DispatchTicket { ticket_path, .. } => {
            let args = crate::dispatch::DispatchArgs {
                target: ticket_path.clone(),
                dispatch_reason: Some("initial".to_string()),
                ..base_args()
            };
            let deferred =
                run_dispatch_and_record(cfg, "dispatch_ticket", action.work_id(), &args)?;
            Ok(deferred.unwrap_or_else(|| format!("Dispatched ticket '{ticket_path}'")))
        }
        NextAction::DecomposeIssue {
            ticket_path,
            work_id,
            title,
            ..
        } => pm::execute(
            cfg,
            profile_name,
            ticket_path,
            work_id,
            title.as_deref(),
            skip_validation_gate,
            route_admission.clone(),
        ),
        NextAction::ReconcilePmParent {
            work_id,
            source_issue_number,
            plan_fingerprint,
            child_issue_numbers,
            ..
        } => pm::reconcile_parent(
            cfg,
            profile_name,
            work_id,
            source_issue_number,
            plan_fingerprint,
            child_issue_numbers,
        ),
        NextAction::Retry { ticket_path, .. } => {
            let args = crate::dispatch::DispatchArgs {
                target: ticket_path.clone(),
                dispatch_reason: Some("retry".to_string()),
                ..base_args()
            };
            let deferred = run_dispatch_and_record(cfg, "retry", action.work_id(), &args)?;
            Ok(deferred.unwrap_or_else(|| format!("Retried ticket '{ticket_path}'")))
        }
        NextAction::Escalate { ticket_path, .. } => {
            let args = crate::dispatch::DispatchArgs {
                target: ticket_path.clone(),
                escalate: true,
                dispatch_reason: Some("escalate".to_string()),
                ..base_args()
            };
            let deferred = run_dispatch_and_record(cfg, "escalate", action.work_id(), &args)?;
            Ok(deferred.unwrap_or_else(|| format!("Escalated ticket '{ticket_path}'")))
        }
        NextAction::WaitUntil { until, reason } => Ok(format!("Waiting until {until} ({reason})")),
        NextAction::HumanRequired {
            work_id: _,
            reason,
            reference,
            reason_code,
        } => Ok(format!(
            "Human required: {reason}{}{}",
            reference
                .as_deref()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default(),
            reason_code
                .as_deref()
                .map(|c| format!(" [code={c}]"))
                .unwrap_or_default()
        )),
        NextAction::NoOp { reason } => Ok(format!("No action: {reason}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redispatch_context_is_bounded_owned_and_absent_for_new_work() {
        let tmp = tempfile::tempdir().unwrap();
        let mut profile = crate::config::tests::test_profile_for_notifications();
        profile.artifact_root = tmp.path().display().to_string();
        let mut entry =
            crate::ledger::LedgerEntry::new("test", &profile, "codex", "fix", "ticket", None, None);
        entry.work_id = Some("TICKET-243".into());
        entry.failure_class = Some("validation_failure".into());
        entry.failure_stage = Some("post_validation".into());
        entry.error_summary = Some("retained retry fact ".repeat(10_000));
        let cfg = crate::config::GahConfig {
            context: Default::default(),
            defaults: Default::default(),
            profiles: std::collections::HashMap::from([("test".into(), profile)]),
        };
        let entries = vec![entry];
        let mut contexts = Vec::new();
        for action in [
            NextAction::Retry {
                work_id: "TICKET-243".into(),
                ticket_path: "ticket".into(),
                reason: "retry".into(),
            },
            NextAction::Escalate {
                work_id: "TICKET-243".into(),
                ticket_path: "ticket".into(),
                reason: "escalate".into(),
            },
        ] {
            contexts.push(
                action_prior_context(&cfg, "test", &action, &entries)
                    .unwrap()
                    .unwrap(),
            );
        }
        let initial = NextAction::DispatchTicket {
            ticket_path: "ticket".into(),
            work_id: Some("TICKET-243".into()),
            recommended_backend: None,
            recommended_model: None,
            reason: "initial".into(),
        };
        assert!(action_prior_context(&cfg, "test", &initial, &entries)
            .unwrap()
            .is_none());
        drop(entries);
        for context in contexts {
            assert!(context.len() <= 4_096);
            assert!(context.contains("retained retry fact"));
        }
    }
}
