//! Resolve a native factory request through the ordinary routing engine, using
//! central history/policy and the selected worker's exact account availability.
use super::*;
use crate::native_handoff::{JobRequest, WorkerCatalog};
use crate::routing::RouteRequest;
use anyhow::{ensure, Context};

pub(crate) fn plan_native(
    cfg: &GahConfig,
    request: &JobRequest,
    catalog: &WorkerCatalog,
) -> Result<JobRequest> {
    ensure!(
        crate::node_role::NodeRoleStatus::resolve(&cfg.defaults)?.role
            == crate::node_role::NodeRole::Central
            && cfg.defaults.factory_enabled != Some(false),
        "Native planning requires the active central factory"
    );
    let args = &request.args;
    ensure!(
        matches!(args.mode.as_str(), "fix" | "review")
            && !args.dry_run
            && !args.prod
            && !args.current_branch
            && !args.issue_intake_override
            && !args.skip_validation_gate
            && !args.allow_draft_fail
            && args.mr.is_none()
            && args.oh_profile.is_none()
            && args.retries == 2,
        "Native planning requires an ordinary fix/review controller request"
    );
    let profile = config::get_profile(cfg, &args.profile)?;
    let work_id = args
        .work_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .context("Native plan requires work identity")?;
    let mut current = LedgerEntry::new(
        &args.profile,
        profile,
        &args.backend,
        &args.mode,
        &args.target,
        args.run_id.clone(),
        None,
    );
    current.work_id = Some(work_id.into());
    let mut runtime = attempts::routing_runtime_state(cfg, &current)?;
    catalog.apply(&args.profile, profile, &catalog.node_id, &mut runtime)?;
    let mut planned = request.clone();
    let metadata = if args.mode == "fix" {
        let target = if args.existing_branch.is_some() {
            work_id
        } else {
            args.target.as_str()
        };
        let issue = issues::resolve_target_to_issue_or_string(profile, target, false)?;
        if let Some(issue) = issue {
            Some(issues::parse_ticket_metadata_from_issue(&issue))
        } else {
            let path = std::path::Path::new(target);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::path::Path::new(&profile.local_path).join(path)
            };
            issues::parse_ticket_metadata(&path)?
        }
    } else {
        None
    };
    let mut selected = None;
    if args.mode == "review" {
        let branch = args
            .branch
            .as_deref()
            .context("Native review requires its selected branch")?;
        let generation = args
            .expected_review_generation
            .as_deref()
            .context("Native review requires its selected generation")?;
        if let Some(reason) = review::policy::review_escalation_reason(
            cfg,
            profile,
            &args.profile,
            branch,
            Some(generation),
        ) {
            selected = if reason == "review_output_invalid" {
                review::policy::next_review_candidate(
                    cfg,
                    profile,
                    &args.profile,
                    branch,
                    None,
                    Some(generation),
                    &crate::availability::resolve_state_path(),
                )
            } else {
                review::policy::next_escalatory_reviewer(
                    cfg,
                    profile,
                    &args.profile,
                    branch,
                    None,
                    Some(generation),
                    &crate::availability::resolve_state_path(),
                )
            };
            ensure!(
                selected.is_some(),
                "Native review escalation has no untried reviewer"
            );
        }
    }
    let requested_backend = selected
        .as_ref()
        .map(|candidate| candidate.backend.as_str())
        .unwrap_or_else(|| config::canonical_backend_name(&args.backend));
    let requested_model = match selected.as_ref() {
        Some(candidate) => candidate.model.as_deref(),
        None => args.model.as_deref(),
    };
    if args.mode == "review" {
        review::policy::ensure_pinned_review_route(args, requested_backend, requested_model)?;
    }
    let request = RouteRequest {
        mode: &args.mode,
        requested_backend,
        requested_instance: args.backend_instance.as_deref(),
        requested_model,
        recommended_backend: metadata
            .as_ref()
            .and_then(|meta| meta.recommended_backend.as_deref()),
        recommended_model: metadata
            .as_ref()
            .and_then(|meta| meta.recommended_model.as_deref()),
        session_id: args.run_id.as_deref(),
        usage_summary: None,
        last_failure_class: args.escalate.then_some("agent_failure"),
        exact_route_required: selected.is_some() || args.backend != "auto",
    };
    let task = crate::routing::TaskRoutingContext {
        task_class: metadata
            .as_ref()
            .and_then(|meta| meta.task_class.as_deref()),
        difficulty: metadata
            .as_ref()
            .and_then(|meta| meta.difficulty.as_deref()),
        risk: metadata.as_ref().and_then(|meta| meta.risk.as_deref()),
    };
    let route = crate::routing::decide_for_task_with_state(
        &cfg.defaults,
        profile,
        request,
        task,
        &runtime,
    )?;
    ensure!(
        !route.human_required
            && route
                .identity
                .effective_model
                .as_deref()
                .is_some_and(|model| !model.is_empty()
                    && !matches!(model.to_ascii_lowercase().as_str(), "auto" | "default")),
        "Native plan requires a concrete permitted model"
    );
    if args.mode == "review" {
        ensure!(
            review::policy::check_review_budget(
                cfg,
                profile,
                &args.profile,
                Some(work_id),
                &route,
                args.expected_review_generation.as_deref()
            )?
            .is_none(),
            "Native review budget holds this work"
        );
    }
    planned.args.backend = route.identity.logical_backend;
    planned.args.model = route.identity.effective_model;
    planned.args.backend_instance = route
        .identity
        .explicit_instance
        .then_some(route.identity.backend_instance);
    planned.args.route_admission = None;
    let mut safe = serde_json::to_value(&planned)?;
    crate::redact::redact_json_value(&mut safe);
    ensure!(
        safe == serde_json::to_value(&planned)?,
        "Native plan contains secret-bearing request evidence"
    );
    Ok(planned)
}
