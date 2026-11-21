//! A native retry may finish an acknowledged implementation without authorizing
//! another backend attempt. The receipt identifies the work and its owner.
use super::{issues, publish, DispatchArgs};
use crate::{
    config::{GahConfig, Profile},
    ledger::LedgerEntry,
    native_handoff::JobRequest,
};
use anyhow::{ensure, Context, Result};

pub(crate) const REASON: &str = "recover_publication";

pub(super) fn history(
    cfg: &GahConfig,
    profile: &Profile,
    args: &DispatchArgs,
) -> Result<Vec<LedgerEntry>> {
    crate::ledger::read_matching_entries(cfg, |e| {
        e.profile == args.profile
            && e.work_id == args.work_id
            && e.repo_id == profile.repo_id
            && e.repo == profile.repo
            && e.provider == profile.provider
    })
}

/// Do not resurrect earlier implementations past an intervening publication,
/// human decision or replacement commit for the same issue.
pub(super) fn source<'a>(
    entries: &'a [LedgerEntry],
    args: &DispatchArgs,
    profile: &Profile,
) -> Option<&'a LedgerEntry> {
    let number = args.target.trim_start_matches('#');
    if !matches!(args.mode.as_str(), "fix" | "improve")
        || args.existing_branch.is_some()
        || args.branch.is_some()
        || number.parse::<u64>().ok()? == 0
        || args.work_id.as_deref()? != format!("#{number}")
    {
        return None;
    }
    // What earlier recoveries of the implementation reached below did. The
    // walk is newest first, so they are all seen before it.
    let mut refused = false;
    let mut pushed = false;
    for e in entries.iter().rev().filter(|e| {
        e.profile == args.profile
            && e.work_id == args.work_id
            && e.repo_id == profile.repo_id
            && e.repo == profile.repo
            && e.provider == profile.provider
    }) {
        if e.human_required || e.mr_created || e.mr_url.is_some() {
            return None;
        }
        if e.dispatch_reason.as_deref() == Some(REASON) {
            // A recovery that only lacked central's permission decided
            // nothing and is repeated. One that failed any other way before
            // publishing is treated as refused and is not repeated. Most
            // often other work has merged and the preserved commit no longer
            // sits on the target, which no repetition can change. A passing
            // provider error is treated the same on purpose, not told apart
            // by its message: redoing the work once costs less than
            // repeating a refusal until the issue is parked.
            pushed |= e.push_attempted;
            refused |= !e.push_attempted && !authority_held(e);
            continue;
        }
        if e.commit_created || e.push_attempted || e.mr_attempted {
            // A refused recovery used to be offered again unchanged until
            // the issue was parked as a repeated setup failure with finished
            // work behind it. Leave the issue to an ordinary retry instead,
            // unless a push was ever attempted, by the implementation itself
            // or by one of its recoveries: the branch may then be on the
            // remote without its merge request, what remains is safe to
            // repeat, and redoing the work would orphan that branch.
            if refused && !pushed && !e.push_attempted {
                return None;
            }
            let checkpoint =
                e.resumable_checkpoint_sha.is_some() && e.resumable_checkpoint_branch == e.branch;
            let before_push = !e.push_attempted && !e.mr_attempted && authority_held(e);
            return (e.provider == "gitlab"
                && e.mode == args.mode
                && e.remote_execution.is_some()
                && matches!(
                    e.dispatch_reason.as_deref(),
                    Some("initial" | "retry" | "escalate")
                )
                && e.source_issue_number.as_deref() == Some(number)
                && e.commit_created
                && e.validation_result.as_deref() == Some("passed")
                && e.failure_class.is_some()
                && (checkpoint || before_push))
                .then_some(e);
        }
    }
    None
}

/// The job finished but central's permission for its next step never arrived.
fn authority_held(e: &LedgerEntry) -> bool {
    e.failure_class.as_deref() == Some("harness_error")
        && e.error_summary
            .as_deref()
            .is_some_and(|s| s.starts_with("Native operation authority is held"))
}

pub(super) fn plan(request: &mut JobRequest, e: &LedgerEntry, node: &str) -> Result<()> {
    ensure!(
        e.remote_execution
            .as_ref()
            .is_some_and(|r| r.node_id == node),
        "Preserved publication belongs to another worker"
    );
    request.publication_source = Some(
        e.session_id
            .as_deref()
            .context("Missing original implementation run")?
            .into(),
    );
    request.args.dispatch_reason = Some(REASON.into());
    request.args.backend = e.effective_backend.clone();
    request.args.model = e.effective_model.clone();
    request.args.backend_instance = None;
    Ok(())
}

pub(crate) fn verify<'a>(
    request: &JobRequest,
    entries: &'a [LedgerEntry],
    profile: &Profile,
    node: &str,
) -> Result<&'a LedgerEntry> {
    let args = &request.args;
    let candidate =
        source(entries, args, profile).context("No acknowledged unpublished implementation")?;
    ensure!(
        args.dispatch_reason.as_deref() == Some(REASON)
            && !args.escalate
            && request.publication_source.is_some()
            && request.publication_source == candidate.session_id
            && candidate
                .remote_execution
                .as_ref()
                .is_some_and(|r| r.node_id == node)
            && candidate.effective_backend == args.backend
            && candidate.effective_model == args.model
            && args.backend_instance.is_none()
            && publish::publishing_allows_publish(profile),
        "Publication source, worker or policy changed"
    );
    Ok(candidate)
}

#[cfg(target_os = "linux")]
pub(super) fn run(
    cfg: &GahConfig,
    profile: &Profile,
    args: &DispatchArgs,
    session: &std::path::Path,
    ledger: &mut LedgerEntry,
) -> Result<()> {
    use crate::{native_handoff::execution, worktree};
    let (request, node) = execution::publication_context()?;
    let entries = history(cfg, profile, args)?;
    let source = verify(request, &entries, profile, node)?;
    crate::policy::enforce_policy(profile, "open-draft-pr")?;
    crate::policy::enforce_policy(profile, "git-push")?;
    let branch = source
        .branch
        .as_deref()
        .context("Missing preserved branch")?;
    let number = source
        .source_issue_number
        .as_deref()
        .context("Missing source issue")?;
    ensure!(
        branch.starts_with(&format!("gah/{}-", profile.repo_id)),
        "Unexpected publication branch"
    );
    let issue = issues::fetch_issue_details(profile, number, false)?;
    ensure!(
        source.work_title.as_deref() == Some(&issue.title),
        "Source issue title changed"
    );
    let repo = std::path::Path::new(&profile.local_path);
    let sha = preserved_head(repo, source, branch, number)?;
    worktree::fetch_origin(repo)?;
    let base = format!("refs/remotes/origin/{}", profile.default_target_branch);
    let base_sha = worktree::git(&["rev-parse", "--verify", &base], repo)?;
    ensure!(
        worktree::git(&["merge-base", &base_sha, &sha], repo)? == base_sha && base_sha != sha,
        "Preserved implementation needs validation against a changed target"
    );
    let wt = session.join("publication-worktree");
    worktree::git(
        &[
            "worktree",
            "add",
            "--detach",
            wt.to_str().context("Invalid worktree path")?,
            &sha,
        ],
        repo,
    )?;
    let result = (|| {
        let cache = crate::build_cache::ScopedCargoTarget::acquire(&profile.artifact_root)?;
        ensure!(
            profile
                .validation_commands
                .iter()
                .any(|s| !s.trim().is_empty()),
            "No publication validation commands"
        );
        crate::validation_runner::validate(
            &profile.validation_commands,
            &wt,
            &cache.environment(),
            std::time::Duration::from_secs(profile.validation_timeout_seconds()),
        )?;
        ensure!(
            !worktree::has_uncommitted_changes(&wt)?
                && worktree::git(&["rev-parse", "HEAD"], &wt)? == sha,
            "Validation changed the preserved checkout"
        );
        publish::enforce_generated_artifact_policy(profile, ledger, &wt)?;
        ledger.branch = Some(branch.into());
        ledger.source_issue_number = Some(number.into());
        ledger.work_title = source.work_title.clone();
        ledger.validation_result = Some("passed".into());
        ledger.effective_backend = source.effective_backend.clone();
        ledger.effective_model = source.effective_model.clone();
        super::metrics::apply_diff_stats(ledger, &wt, &profile.default_target_branch);
        super::native_guard::publication(profile, || {
            publish::ensure_issue_open_for_publish(profile, &issue)?;
            let existing = existing_mr(profile, branch, &sha, number)?;
            let target = worktree::git(
                &[
                    "ls-remote",
                    "--exit-code",
                    "origin",
                    &format!("refs/heads/{}", profile.default_target_branch),
                ],
                repo,
            )?;
            ensure!(
                target.split_whitespace().next() == Some(&base_sha),
                "Target changed during validation"
            );
            if let Some(url) = existing {
                ledger.mr_url = Some(url);
                return Ok(());
            }
            ledger.push_attempted = true;
            worktree::publish_preserved_commit(
                repo,
                branch,
                &profile.default_target_branch,
                &sha,
                &profile.push_url()?,
                &crate::provider::push_token(profile),
            )?;
            ledger.push_succeeded = true;
            let ticket = issues::parse_ticket_metadata_from_issue(&issue);
            let title = publish::build_mr_title("fix", &profile.repo_id, false, Some(&ticket));
            let summary = format!(
                "Recovered and revalidated commit `{sha}` from implementation `{}`.",
                source.session_id.as_deref().unwrap_or_default()
            );
            let body = publish::build_fix_or_improve_mr_body(
                "fix",
                Some(&ticket),
                &publish::MrRenderContext {
                    backend: &source.effective_backend,
                    model: source.effective_model.as_deref().unwrap_or("unknown"),
                    branch,
                    target_branch: &profile.default_target_branch,
                    validation_commands: &profile.validation_commands,
                    ledger,
                    backend_summary: &summary,
                },
                true,
            );
            ledger.mr_attempted = true;
            ledger.mr_url =
                Some(crate::provider::create_draft_mr(profile, branch, &title, &body)?.url);
            ledger.mr_created = true;
            Ok(())
        })
    })();
    worktree::cleanup(&wt, repo);
    result
}

#[cfg(target_os = "linux")]
fn preserved_head(
    repo: &std::path::Path,
    e: &LedgerEntry,
    branch: &str,
    number: &str,
) -> Result<String> {
    let head = crate::worktree::git(
        &[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
        repo,
    )?;
    let pinned = match &e.resumable_checkpoint_sha {
        Some(sha) if e.resumable_checkpoint_branch.as_deref() == Some(branch) => sha.clone(),
        // Old receipts named only a branch. Require the operator-preserved
        // issue ref too, never infer a historical commit from a movable tip.
        None => crate::worktree::git(
            &[
                "rev-parse",
                "--verify",
                &format!("refs/parasaur/recovery/issue-{number}^{{commit}}"),
            ],
            repo,
        )
        .context("Legacy implementation has no pinned recovery commit")?,
        _ => anyhow::bail!("Publication checkpoint branch mismatch"),
    };
    ensure!(head == pinned, "Preserved branch changed since validation");
    Ok(head)
}

#[cfg(target_os = "linux")]
fn existing_mr(profile: &Profile, branch: &str, sha: &str, number: &str) -> Result<Option<String>> {
    let project = crate::provider::gitlab_project_id(profile)?;
    let rows = crate::provider::gitlab_api(
        profile,
        &format!("projects/{project}/merge_requests"),
        "GET",
        &[
            ("scope", "all"),
            ("state", "all"),
            ("source_branch", branch),
        ],
    )?;
    let rows = rows.as_array().context("Invalid merge request lookup")?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mr = &rows[0];
    ensure!(
        rows.len() == 1
            && mr["state"] == "opened"
            && mr["source_branch"] == branch
            && mr["sha"] == sha
            && mr["target_branch"] == profile.default_target_branch
            && mr["source_project_id"].as_u64().is_some()
            && mr["source_project_id"] == mr["target_project_id"]
            && mr["description"]
                .as_str()
                .is_some_and(|s| s.lines().any(|l| l.trim() == format!("Closes #{number}"))),
        "Prior merge request does not match this publication"
    );
    Ok(Some(
        mr["web_url"]
            .as_str()
            .context("Existing merge request missing URL")?
            .to_owned(),
    ))
}
