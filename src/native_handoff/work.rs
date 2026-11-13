//! Revalidate the controller-selected purpose and route using current evidence.
//! This still grants neither ownership nor permission to start a worker.
use super::*;
use crate::controller::NextAction;

/// Require the prepared request to remain the controller's next executable
/// action. Never manufacture an eligible candidate by filtering out blockers,
/// active claims, higher-priority lifecycle work or exhausted retry budgets.
pub fn check_current_work(
    cfg: &GahConfig,
    context: &Path,
    expected: &str,
    node_id: &str,
    origin: &str,
) -> Result<DispatchBinding> {
    check_current(cfg, context, expected, node_id, origin, true, None)
}

/// Recheck the same immutable policy, exact account/model and live routing
/// approvals for a running job. Queue priority is not renewed execution
/// authority: central must also check its consumed start and held native claim.
pub fn check_current_route(
    cfg: &GahConfig,
    context: &Path,
    expected: &str,
    node_id: &str,
    origin: &str,
) -> Result<DispatchBinding> {
    check_current(cfg, context, expected, node_id, origin, false, None)
}

pub(super) fn check_current(
    cfg: &GahConfig,
    context: &Path,
    expected: &str,
    node_id: &str,
    origin: &str,
    require_next_action: bool,
    catalog: Option<&WorkerCheckScope>,
) -> Result<DispatchBinding> {
    let bound = check_current_policy(cfg, context, expected, node_id, origin)?;
    let manifest_bytes = read_blob(&context.join("manifest.json"), None, MAX_JSON)?;
    ensure!(
        format!("{:x}", Sha256::digest(&manifest_bytes)) == expected,
        "Native context changed during work revalidation"
    );
    let manifest: Manifest = read_json(&manifest_bytes)?;
    let request: JobRequest = read_json(&read_blob(
        &context.join("request.json"),
        Some(
            manifest
                .files
                .get("request.json")
                .context("Prepared request is missing")?,
        ),
        MAX_JSON,
    )?)?;
    let args = &request.args;
    let profile = crate::config::get_profile(cfg, &args.profile)?;
    ensure!(
        !args.dry_run
            && !args.prod
            && !args.current_branch
            && !args.issue_intake_override
            && !args.skip_validation_gate
            && !args.allow_draft_fail
            && args.mr.is_none()
            && args.oh_profile.is_none()
            && args.retries == if args.mode == "pm" { 0 } else { 2 },
        "Native factory work cannot override controller execution restrictions"
    );
    ensure!(
        args.backend != "auto"
            && args.model.as_deref().is_some_and(|model| !model.is_empty()
                && !matches!(model.to_ascii_lowercase().as_str(), "auto" | "default")),
        "Native work revalidation requires an exact backend and model"
    );
    let entries = crate::ledger::read_entries(cfg)?;
    if require_next_action {
        let mut snapshot = crate::status::build_snapshot_from_entries(
            cfg,
            &args.profile,
            crate::clock::now_utc(),
            &entries,
        )?;
        if let Some(catalog) = catalog {
            catalog.project(&args.profile, &bound, Some(&mut snapshot))?;
        }
        ensure!(
            !snapshot
                .errors
                .iter()
                .any(|error| error.incomplete_snapshot),
            "Current work evidence is incomplete; native work remains held"
        );
        let action = crate::controller::decide_next_action(&snapshot);
        let generation = match &action {
            NextAction::ReviewMr { branch, .. } => snapshot
                .merge_requests
                .iter()
                .find(|mr| &mr.branch == branch)
                .and_then(|mr| mr.review_generation.as_deref()),
            _ => None,
        };
        ensure!(matches_action(args,&action,generation,&profile.local_path),"Prepared native work is no longer the current controller action ({}); reconcile without starting it",action.kind());
    }
    let mut current = crate::ledger::LedgerEntry::new(
        &args.profile,
        profile,
        &args.backend,
        &args.mode,
        "native-preflight",
        args.run_id.clone(),
        None,
    );
    current.work_id = args.work_id.clone();
    let mut runtime = crate::dispatch::routing_runtime_state_from_entries(&entries, &current);
    if let Some(catalog) = catalog {
        catalog.project(&args.profile, &bound, None)?;
        catalog
            .catalog()
            .apply(&args.profile, profile, node_id, &mut runtime)?;
    }
    let route = crate::routing::decide_with_state(
        &cfg.defaults,
        profile,
        crate::routing::RouteRequest {
            mode: &args.mode,
            requested_backend: &args.backend,
            requested_instance: args.backend_instance.as_deref(),
            requested_model: args.model.as_deref(),
            recommended_backend: None,
            recommended_model: None,
            session_id: args.run_id.as_deref(),
            usage_summary: None,
            last_failure_class: None,
            exact_route_required: true,
        },
        &runtime,
    )?;
    ensure!(
        !route.human_required
            && route.identity.logical_backend == args.backend
            && route.identity.effective_model == args.model
            && args
                .backend_instance
                .as_deref()
                .is_none_or(|id| route.identity.backend_instance == id),
        "Current route differs from the prepared native account/model"
    );
    Ok(bound)
}

fn matches_action(
    args: &DispatchArgs,
    action: &NextAction,
    generation: Option<&str>,
    repo: &str,
) -> bool {
    let work = |id: Option<&str>| {
        id.zip(args.work_id.as_deref())
            .is_some_and(|(left, right)| {
                crate::work_claim::normalize_work_identity(left)
                    == crate::work_claim::normalize_work_identity(right)
            })
    };
    if !work(action.work_id()) {
        return false;
    }
    let target = |value: &str| {
        let number = value.strip_prefix('#').unwrap_or(value);
        if !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) {
            return std::path::PathBuf::from(crate::work_claim::normalize_work_identity(value));
        }
        Path::new(value)
            .strip_prefix(repo)
            .unwrap_or(Path::new(value))
            .to_path_buf()
    };
    match action {
        NextAction::ReviewMr { branch, .. } => {
            args.mode == "review"
                && args.target.is_empty()
                && args.dispatch_reason.as_deref() == Some("review")
                && args.branch.as_ref() == Some(branch)
                && args.existing_branch.is_none()
                && !args.escalate
                && !args.allow_unknown_red_baseline
                && generation.is_some()
                && args.expected_review_generation.as_deref() == generation
        }
        NextAction::FixMr {
            branch,
            review_generation,
            ..
        } => {
            args.mode == "fix"
                && args.dispatch_reason.as_deref() == Some("post_review_repair")
                && args.existing_branch.as_ref() == Some(branch)
                && args.target == *branch
                && args.branch.is_none()
                && !args.escalate
                && args.allow_unknown_red_baseline
                && review_generation.is_some()
                && args.expected_review_generation == *review_generation
        }
        NextAction::DispatchTicket { ticket_path, .. }
        | NextAction::Retry { ticket_path, .. }
        | NextAction::Escalate { ticket_path, .. } => {
            let (reason, escalate) = match action {
                NextAction::DispatchTicket { .. } => ("initial", false),
                NextAction::Retry { .. } => ("retry", false),
                _ => ("escalate", true),
            };
            args.mode == "fix"
                && args.dispatch_reason.as_deref() == Some(reason)
                && args.escalate == escalate
                && !args.allow_unknown_red_baseline
                && args.branch.is_none()
                && args.existing_branch.is_none()
                && args.expected_review_generation.is_none()
                && target(&args.target) == target(ticket_path)
        }
        NextAction::DecomposeIssue { ticket_path, .. } => {
            args.mode == "pm"
                && args.dispatch_reason.as_deref() == Some("pm_decomposition")
                && !args.escalate
                && !args.allow_unknown_red_baseline
                && args.prior_attempt_context.is_none()
                && args.branch.is_none()
                && args.existing_branch.is_none()
                && args.expected_review_generation.is_none()
                && target(&args.target) == target(ticket_path)
        }
        // Provider-only publication has a separate authority contract. Never
        // infer permission to merge or publish from a matching work identity.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repair_and_pm_intents_do_not_imply_merge_or_publication_authority() {
        let (_tmp, _central, _worker, request) = crate::native_handoff::tests::fixture();
        let mut args = request.args;
        args.mode = "pm".into();
        args.target = "#1525".into();
        args.dispatch_reason = Some("pm_decomposition".into());
        args.prior_attempt_context = None;
        args.retries = 0;
        let plan = NextAction::DecomposeIssue {
            ticket_path: "1525".into(),
            work_id: "#1525".into(),
            title: None,
            reason: "fixture".into(),
        };
        assert!(matches_action(&args, &plan, None, "/central/repo"));
        args.mode = "fix".into();
        args.target = "feature".into();
        args.existing_branch = Some("feature".into());
        args.dispatch_reason = Some("post_review_repair".into());
        args.expected_review_generation = Some("selected".into());
        args.allow_unknown_red_baseline = true;
        args.retries = 2;
        let repair = NextAction::FixMr {
            work_id: Some("#1525".into()),
            branch: "feature".into(),
            mr_url: None,
            review_generation: Some("selected".into()),
            reason: "fixture".into(),
        };
        assert!(matches_action(&args, &repair, None, "/central/repo"));
        args.expected_review_generation = Some("stale".into());
        assert!(!matches_action(&args, &repair, None, "/central/repo"));
        args.expected_review_generation = Some("selected".into());
        let merge = NextAction::MergeMr {
            work_id: Some("#1525".into()),
            branch: "feature".into(),
            mr_url: None,
            review_generation: Some("selected".into()),
            reason: "fixture".into(),
        };
        assert!(!matches_action(&args, &merge, None, "/central/repo"));
    }
    #[test]
    fn changed_work_purpose_target_or_review_generation_cannot_start_a_prepared_job() {
        let (_tmp, _central, _worker, request) = crate::native_handoff::tests::fixture();
        let mut args = request.args;
        let action = NextAction::Retry {
            work_id: "1525".into(),
            ticket_path: "docs/task.md".into(),
            reason: "fixture".into(),
        };
        assert!(matches_action(&args, &action, None, "/central/repo"));
        for changed in ["other-work", "other-purpose", "other-target", "escalate"] {
            let mut wrong = args.clone();
            match changed {
                "other-work" => wrong.work_id = Some("#7".into()),
                "other-purpose" => wrong.dispatch_reason = Some("initial".into()),
                "other-target" => wrong.target = "docs/other.md".into(),
                _ => wrong.escalate = true,
            }
            assert!(!matches_action(&wrong, &action, None, "/central/repo"));
        }
        args.mode = "review".into();
        args.target.clear();
        args.branch = Some("feature".into());
        args.dispatch_reason = Some("review".into());
        args.expected_review_generation = Some("generation-a".into());
        let review = NextAction::ReviewMr {
            work_id: Some("#1525".into()),
            branch: "feature".into(),
            mr_url: None,
            reason: "fixture".into(),
        };
        assert!(matches_action(
            &args,
            &review,
            Some("generation-a"),
            "/central/repo"
        ));
        assert!(!matches_action(
            &args,
            &review,
            Some("generation-b"),
            "/central/repo"
        ));
        assert!(!matches_action(&args, &review, None, "/central/repo"));
        let blocked = NextAction::HumanRequired {
            work_id: Some("#1525".into()),
            reason: "review gated".into(),
            reference: None,
            reason_code: None,
        };
        assert!(!matches_action(&args, &blocked, None, "/central/repo"));
    }
}
