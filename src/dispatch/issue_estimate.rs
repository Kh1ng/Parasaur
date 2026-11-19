//! An optional routing estimate is a separately metered, read-only job. Its
//! result can inform implementation, but it cannot finish or fail the issue.
use super::{attempts, issues, native_guard, DispatchArgs};
use crate::config::{GahConfig, Profile};
use crate::ledger::LedgerEntry;
use crate::routing::{RouteDecision, RouteRequest, RoutingRuntimeState};
use anyhow::Result;

pub(crate) const REASON: &str = "estimate_before_dispatch";

pub(crate) fn is_preliminary(entry: &LedgerEntry) -> bool {
    entry.mode == "estimate" && entry.dispatch_reason.as_deref() == Some(REASON)
}

pub(super) enum Estimate {
    Skip,
    Needed,
    Recorded(String),
}

pub(super) fn for_issue(
    cfg: &GahConfig,
    profile: &Profile,
    args: &DispatchArgs,
    number: &str,
    metadata: &issues::TicketMetadata,
) -> Result<Estimate> {
    if profile
        .routing
        .estimate_issue_difficulty
        .or(cfg.defaults.routing.estimate_issue_difficulty)
        != Some(true)
        || metadata
            .difficulty
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(Estimate::Skip);
    }
    let work = format!("#{number}");
    let mut previous = None;
    crate::ledger::visit_entries(cfg, |entry| {
        if entry.profile != args.profile
            || entry.repo_id != profile.repo_id
            || entry.repo != profile.repo
            || entry.provider != profile.provider
            || (entry.work_id.as_deref() != Some(&work)
                && entry.source_issue_number.as_deref() != Some(number))
        {
            return Ok(());
        }
        if is_preliminary(&entry) {
            previous = Some(match entry.predicted_difficulty {
                Some(value)
                    if matches!(value.as_str(), "easy" | "medium" | "hard")
                        && entry.failure_class.is_none()
                        && entry.validation_result.as_deref() == Some("completed") =>
                {
                    Estimate::Recorded(value)
                }
                _ => Estimate::Skip,
            });
        } else if previous.is_none() && matches!(entry.mode.as_str(), "fix" | "improve" | "handoff")
        {
            previous = Some(Estimate::Skip);
        }
        Ok(())
    })?;
    Ok(previous.unwrap_or(
        if args.branch.is_none()
            && args.existing_branch.is_none()
            && !args.escalate
            && matches!(
                args.dispatch_reason.as_deref(),
                None | Some("initial") | Some(REASON)
            )
        {
            Estimate::Needed
        } else {
            Estimate::Skip
        },
    ))
}

/// Planning the optional call is side-effect free. If no permitted estimator
/// is available the original implementation still runs its own route policy.
pub(super) fn route(
    cfg: &GahConfig,
    profile: &Profile,
    args: &DispatchArgs,
    runtime: &RoutingRuntimeState,
) -> Option<RouteDecision> {
    crate::routing::decide_with_state(
        &cfg.defaults,
        profile,
        RouteRequest {
            mode: "estimate",
            requested_backend: "auto",
            requested_instance: None,
            requested_model: None,
            recommended_backend: None,
            recommended_model: None,
            session_id: args.run_id.as_deref(),
            usage_summary: None,
            last_failure_class: None,
            exact_route_required: false,
        },
        runtime,
    )
    .ok()
    .filter(|route| {
        !route.human_required
            && route
                .identity
                .effective_model
                .as_deref()
                .is_some_and(|model| {
                    !model.is_empty()
                        && !matches!(model.to_ascii_lowercase().as_str(), "auto" | "default")
                })
    })
}

pub(super) fn request(args: &DispatchArgs, number: &str, route: RouteDecision) -> DispatchArgs {
    DispatchArgs {
        mode: "estimate".into(),
        dispatch_reason: Some(REASON.into()),
        backend: route.identity.logical_backend,
        backend_instance: route
            .identity
            .explicit_instance
            .then_some(route.identity.backend_instance),
        model: route.identity.effective_model,
        target: number.into(),
        work_id: Some(format!("#{number}")),
        ..args.clone()
    }
}

/// A native worker can only run its immutable job. Central schedules its
/// estimate separately, then includes the accepted result in worker history.
pub(super) fn metadata(
    cfg: &GahConfig,
    profile: &Profile,
    args: &DispatchArgs,
    issue: &issues::IssueDetails,
    ledger: &LedgerEntry,
) -> Result<issues::TicketMetadata> {
    let number = &issue.number;
    let mut metadata = issues::parse_ticket_metadata_from_issue(issue);
    match for_issue(cfg, profile, args, number, &metadata)? {
        Estimate::Recorded(value) => metadata.difficulty = Some(value),
        Estimate::Needed
            if !native_guard::active() && !crate::controller::native_dispatch_enabled(cfg)? =>
        {
            let runtime = attempts::routing_runtime_state(cfg, ledger)?;
            if let Some(candidate) = route(cfg, profile, args, &runtime) {
                let mut estimate = request(args, number, candidate);
                estimate.run_id = Some(uuid::Uuid::new_v4().to_string());
                if let Err(error) = super::run(cfg, &estimate) {
                    if error.is::<crate::dispatch_cancel::Cancelled>()
                        || crate::runner::shutdown_requested()
                    {
                        return Err(error);
                    }
                    eprintln!(
                        "Pre-dispatch estimate unavailable; continuing with unknown difficulty."
                    );
                }
                crate::spend_budget::ensure_available(cfg, &args.profile)?;
                if let Estimate::Recorded(value) = for_issue(cfg, profile, args, number, &metadata)?
                {
                    metadata.difficulty = Some(value);
                }
            }
        }
        _ => {}
    }
    Ok(metadata)
}

/// Recheck against the current provider issue and accepted history just before
/// granting a native estimate. Prepared work may have been classified meanwhile.
pub(crate) fn still_needed(
    cfg: &GahConfig,
    profile: &Profile,
    args: &DispatchArgs,
) -> Result<bool> {
    let Some(issue) = issues::resolve_target_to_issue_or_string(profile, &args.target, false)?
    else {
        return Ok(false);
    };
    Ok(matches!(
        for_issue(
            cfg,
            profile,
            args,
            &issue.number,
            &issues::parse_ticket_metadata_from_issue(&issue)
        )?,
        Estimate::Needed
    ))
}
