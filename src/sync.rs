use crate::config::{self, GahConfig};
use crate::job_kind::JobKind;
use crate::provider::{gitlab_api, gitlab_project_id};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

mod repository;
pub(crate) mod requested_changes;
mod review_state;
pub use repository::fetch_repository_mrs;
pub(crate) use review_state::review_metadata_fingerprint;
pub(crate) use review_state::{
    current_review_generation, review_contract_matches, review_generation_matches,
    review_generation_matches_mr,
};
use review_state::{latest_review_for_mr, ledger_info_for_mr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MrFetchScope {
    Active,
    FullHistory,
}

/// TICKET-070: `json = true` prints one JSON array to stdout and nothing
/// else (the existing human-readable output is otherwise unchanged when
/// `json = false`) -- Hermes and other automation should never have to
/// parse pretty-printed terminal output to learn MR state.
pub fn run(cfg: &GahConfig, profile_name: &str, json: bool) -> Result<()> {
    let profile = config::get_profile(cfg, profile_name)?;
    let mrs = fetch_mrs(profile)?;

    if json {
        let entries = crate::ledger::read_entries(cfg).unwrap_or_default();
        let ledger_index = crate::ledger::index_entries_by_work_id(&entries);
        let rows: Vec<SyncMrJson> = mrs
            .iter()
            .map(|mr| sync_mr_to_json(mr, Some(profile_name.to_string()), &ledger_index))
            .collect();
        println!("{}", serde_json::to_string(&rows)?);
        return Ok(());
    }

    println!("Profile: {}", profile_name);
    for mr in &mrs {
        let class = classify(mr);
        println!(
            "{}  {}  {}  {}",
            class,
            mr.branch,
            mr.title,
            mr.url.as_deref().unwrap_or("")
        );
        println!("  recommended: {}", recommended_action(class));
    }
    Ok(())
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecommendedAction {
    ReuseBranch,
    HumanMergeDecision,
    RunReview,
    None,
    InspectBranch,
    InspectManually,
}

impl RecommendedAction {
    pub fn from_class(class: &str) -> Self {
        match class {
            "CI_FAILED" | "NEEDS_FIX" => RecommendedAction::ReuseBranch,
            "READY_FOR_HUMAN" => RecommendedAction::HumanMergeDecision,
            "NEEDS_REVIEW" => RecommendedAction::RunReview,
            "MERGED" | "CLOSED_UNMERGED" => RecommendedAction::None,
            "STALE" => RecommendedAction::InspectBranch,
            _ => RecommendedAction::InspectManually,
        }
    }
}

/// Machine-readable row for `gah sync --json`. Field set is the ticket's
/// "at minimum" list: profile, branch, MR identifier, MR URL, state, draft,
/// merge status if available, classification, recommended next action.
#[derive(Debug, Serialize, Clone)]
pub struct SyncMrJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_id: Option<String>,
    pub id: Option<String>,
    pub url: Option<String>,
    pub state: Option<String>,
    pub draft: bool,
    pub merge_status: Option<String>,
    pub merged: bool,
    pub ci_passed: bool,
    /// TICKET-198: human-readable PR/MR title, surfaced on the dashboard's
    /// Recently Merged panel instead of the raw branch name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// TICKET-198: merge timestamp (RFC3339) for merged MRs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_at: Option<String>,
    /// TICKET-198: backend/model that produced the merge, from the ledger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_backend: Option<String>,
    /// TICKET-198: model that produced the merge, from the ledger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_model: Option<String>,
    /// TICKET-198: review verdict recorded for the merge, from the ledger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_verdict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_gate_reason: Option<String>,
    /// Current provider opinions can veto merging without authorizing a repair.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_review_block_reason: Option<String>,
    /// Immutable source and independently-versioned lifecycle generation used
    /// to decide whether historical review-derived gates remain authoritative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_sha: Option<String>,
    /// Last completed head receipt; suppresses review calls without granting merge authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reviewed_head: Option<String>,
    /// Provider-created commit that landed a merged PR. Kept separate from
    /// `source_sha`, which remains the reviewed head/source identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_commit_sha: Option<String>,
    pub review_contract_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_generation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_generation_status: Option<String>,
    /// Non-terminal / unknown CI (running/pending/missing) for which there
    /// is no defined next controller action yet. GitHub is never `ci_pending`
    /// (see `SyncMr::ci_pending`); this mirrors the typed field on `SyncMr`.
    pub ci_pending: bool,
    pub classification: String,
    pub recommended_action: RecommendedAction,
}

#[derive(Debug, Clone)]
pub struct SyncMr {
    pub title: String,
    pub body: Option<String>,
    pub branch: String,
    pub labels: Vec<String>,
    pub url: Option<String>,
    pub id: Option<String>,
    pub state: Option<String>,
    pub draft: bool,
    /// Immutable source commit currently attached to the provider MR.
    pub source_sha: Option<String>,
    /// Provider-created commit that landed this MR, when merged.
    pub merge_commit_sha: Option<String>,
    pub merge_status: Option<String>,
    pub merged: bool,
    pub updated_at: Option<String>,
    /// TICKET-198: merge timestamp (RFC3339) for merged MRs, surfaced on the
    /// dashboard's Recently Merged panel. Empty for unmerged MRs.
    pub merged_at: Option<String>,
    pub ci_failed: bool,
    /// True only when CI has *conclusively* passed (every check/pipeline
    /// terminal and green) -- distinct from `!ci_failed`, which is also
    /// true while CI is still pending/running or absent entirely. Gates
    /// auto-merge (TICKET-127): merging on "not failed yet" would merge
    /// mid-pipeline.
    pub ci_passed: bool,
    /// Issue #156: CI is non-terminal / unknown (running/pending/missing) --
    /// there is no conclusive pass or failure yet. Distinct from `!ci_passed`
    /// (which is also true once CI has *failed*). GitHub never sets this
    /// (its classification handles pending via `github_ci_passed`/`_failed`);
    /// only GitLab's `head_pipeline.status` gap populates it.
    pub ci_pending: bool,
    /// TICKET-096: populated from an authoritative `TICKET-NNN` token in
    /// the PR/MR title (see `build_mr_title` in dispatch.rs), not from a
    /// separate reconciliation structure.
    pub work_id: Option<String>,
}

/// Extract the canonical provider issue identity (`#NNN`) from new PR/MR
/// titles, while continuing to read legacy `TICKET-NNN` titles. No attempt
/// to disambiguate authoritative vs stale IDs happens here -- that check
/// already happened when the title was generated.
pub(crate) fn extract_work_id_from_title(title: &str) -> Option<String> {
    if let Some(issue_id) = title.split('#').skip(1).find_map(|rest| {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        (!digits.is_empty()).then(|| format!("#{digits}"))
    }) {
        return Some(issue_id);
    }

    if let Some(idx) = title.find("TICKET-") {
        let rest = &title[idx + "TICKET-".len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return Some(format!("TICKET-{digits}"));
        }
    }
    None
}

pub fn fetch_mrs(profile: &config::Profile) -> Result<Vec<SyncMr>> {
    fetch_mrs_for_scope(profile, MrFetchScope::FullHistory, true)
}

/// Fetch only merge requests that can drive a controller action. Recurring
/// status/controller observations must use this bounded path; full history is
/// reserved for explicit synchronization, reconciliation, and pruning.
pub fn fetch_active_mrs(profile: &config::Profile) -> Result<Vec<SyncMr>> {
    fetch_mrs_for_scope(profile, MrFetchScope::Active, true)
}

fn fetch_mrs_for_scope(
    profile: &config::Profile,
    scope: MrFetchScope,
    filter_gah_branches: bool,
) -> Result<Vec<SyncMr>> {
    use crate::provider_kind::ProviderKind;
    let mut mrs = match ProviderKind::parse(&profile.provider) {
        Ok(ProviderKind::Github) if scope == MrFetchScope::Active => {
            repository::fetch_active_github_mrs(profile, filter_gah_branches)
        }
        Ok(ProviderKind::Github) => {
            repository::fetch_historical_github_mrs(profile, filter_gah_branches)
        }
        Ok(ProviderKind::Gitlab) => gitlab_mrs(profile, scope, filter_gah_branches),
        Err(_) => anyhow::bail!("unsupported provider: {}", profile.provider),
    }?;
    if scope == MrFetchScope::Active {
        mrs.retain(|mr| {
            mr.state.as_deref().is_some_and(|state| {
                state.eq_ignore_ascii_case("open") || state.eq_ignore_ascii_case("opened")
            }) && !mr.merged
        });
    }
    Ok(mrs)
}

pub fn classify(mr: &SyncMr) -> &'static str {
    if mr.merged {
        return "MERGED";
    }
    if is_closed_unmerged(mr.state.as_deref(), mr.merged) {
        return "CLOSED_UNMERGED";
    }
    if mr.ci_failed {
        return "CI_FAILED";
    }
    if mr.labels.iter().any(|l| l == "gah-needs-fix") {
        return "NEEDS_FIX";
    }
    if mr.labels.iter().any(|l| l == "gah-ready-for-human") {
        return "READY_FOR_HUMAN";
    }
    if mr
        .labels
        .iter()
        .any(|l| l == "gah-human-review" || l == "gah-review-weak" || l == "gah-review-escalating")
    {
        return "NEEDS_REVIEW";
    }
    if is_stale(mr.updated_at.as_deref()) {
        return "STALE";
    }
    if mr.branch.starts_with("gah/") {
        return "NEEDS_REVIEW";
    }
    "UNKNOWN"
}

pub fn recommended_action(class: &str) -> &'static str {
    match class {
        "CI_FAILED" => "reuse same branch/MR for a fix run",
        "NEEDS_FIX" => "reuse same branch/MR for a fix run",
        "READY_FOR_HUMAN" => "human review and merge decision",
        "NEEDS_REVIEW" => "run review or request human review",
        "MERGED" => "none",
        "CLOSED_UNMERGED" => "none",
        "STALE" => "inspect before reusing branch",
        _ => "inspect manually",
    }
}

/// TICKET-198: build the machine-readable `SyncMrJson` row, enriching it with
/// the ledger-derived backend/model and review verdict for the merge (when a
/// `work_id` is available to join on). Centralized so `gah sync --json` and
/// `gah status --json` emit an identical shape.
pub fn sync_mr_to_json(
    mr: &SyncMr,
    profile: Option<String>,
    ledger: &crate::ledger::LedgerEntriesByWorkId,
) -> SyncMrJson {
    let mut class = classify(mr);
    let (effective_backend, effective_model, mut review_verdict, mut review_gate_reason) =
        ledger_info_for_mr(ledger, mr);
    let latest_review = latest_review_for_mr(ledger, mr);
    let latest_review_is_current =
        latest_review.is_some_and(|latest| review_contract_matches(latest, mr));
    let review_generation = if latest_review_is_current {
        latest_review.and_then(|entry| entry.review_generation.clone())
    } else {
        current_review_generation(mr)
    };
    let review_generation_status = latest_review.and_then(|latest| {
        (!latest_review_is_current).then(|| {
            if latest.review_contract_version
                != Some(crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION)
            {
                format!(
                    "superseded review contract {} -> {}",
                    latest
                        .review_contract_version
                        .map_or_else(|| "legacy".to_string(), |version| version.to_string()),
                    crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION
                )
            } else if latest.review_generation.is_none() {
                "superseded review missing generation identity".to_string()
            } else {
                "superseded review because source or provider metadata changed".to_string()
            }
        })
    });
    if !matches!(class, "MERGED" | "CLOSED_UNMERGED" | "CI_FAILED") {
        if let Some(latest) = latest_review {
            if !latest_review_is_current {
                // The provider label reflects an opinion about older
                // metadata or a superseded review contract. Both require a
                // fresh bounded review, not repair or a terminal human gate.
                class = "NEEDS_REVIEW";
                review_verdict = None;
                review_gate_reason = review_generation_status.clone();
            } else {
                // Provider label mutations can fail or lag after a completed
                // review. Reconcile from the exact-branch, exact-metadata
                // structured verdict so stale NEEDS_FIX labels cannot launch
                // a repair that preflight must reject.
                class = match latest.review_verdict.as_deref() {
                    Some("APPROVE") => "READY_FOR_HUMAN",
                    Some("NEEDS_FIX" | "REJECT") => "NEEDS_FIX",
                    Some("HUMAN_REVIEW") if latest.human_required => "READY_FOR_HUMAN",
                    Some("HUMAN_REVIEW") => "NEEDS_REVIEW",
                    Some("REVIEW_OUTPUT_INVALID") => "NEEDS_REVIEW",
                    _ => class,
                };
            }
        }
    }
    SyncMrJson {
        profile,
        branch: mr.branch.clone(),
        work_id: mr.work_id.clone(),
        id: mr.id.clone(),
        url: mr.url.clone(),
        title: Some(mr.title.clone()),
        state: mr.state.clone(),
        draft: mr.draft,
        merge_status: mr.merge_status.clone(),
        merged: mr.merged,
        merged_at: mr.merged_at.clone(),
        ci_passed: mr.ci_passed,
        ci_pending: mr.ci_pending,
        effective_backend,
        effective_model,
        review_verdict,
        review_gate_reason,
        merge_review_block_reason: None,
        source_sha: mr.source_sha.clone(),
        last_reviewed_head: None,
        merge_commit_sha: mr.merge_commit_sha.clone(),
        review_contract_version: crate::ledger::REVIEW_CONTRACT_VERSION,
        review_generation,
        review_generation_status,
        classification: class.to_string(),
        recommended_action: RecommendedAction::from_class(class),
    }
}

fn is_closed_unmerged(state: Option<&str>, merged: bool) -> bool {
    !merged && matches!(state.map(|s| s.to_ascii_lowercase()), Some(ref s) if s == "closed")
}

fn is_stale(updated_at: Option<&str>) -> bool {
    let Some(updated_at) = updated_at else {
        return false;
    };
    let cutoff = (OffsetDateTime::now_utc() - Duration::days(14))
        .format(&Rfc3339)
        .unwrap_or_default();
    updated_at < cutoff.as_str()
}

#[derive(Debug, Deserialize, PartialEq)]
struct GithubPr {
    title: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(rename = "headRefName")]
    head_ref_name: String,
    #[serde(default)]
    #[serde(rename = "headRefOid")]
    head_ref_oid: Option<String>,
    #[serde(default)]
    #[serde(rename = "mergeCommit")]
    merge_commit: Option<GithubCommit>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    labels: Vec<GithubLabel>,
    #[serde(default)]
    number: Option<i64>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    #[serde(rename = "isDraft")]
    is_draft: bool,
    #[serde(default)]
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: Option<String>,
    #[serde(default)]
    #[serde(rename = "mergedAt")]
    merged_at: Option<String>,
    #[serde(default)]
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
    #[serde(default)]
    #[serde(rename = "statusCheckRollup")]
    status_check_rollup: Option<Vec<GithubCheck>>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct GithubCommit {
    oid: String,
}

impl GithubPr {
    fn merge_commit_sha(&self) -> Option<String> {
        self.merge_commit.as_ref().map(|commit| commit.oid.clone())
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct GithubLabel {
    name: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct GithubCheck {
    #[serde(default)]
    conclusion: Option<String>,
}

fn github_ci_failed(checks: Option<&[GithubCheck]>) -> bool {
    crate::github_ci::classify(
        checks
            .unwrap_or_default()
            .iter()
            .map(|check| check.conclusion.as_deref()),
    ) == crate::github_ci::GithubCiStatus::Failed
}

/// Every check present, all terminal, none failed -- as opposed to
/// `!github_ci_failed`, which is also true while checks are still pending
/// (`conclusion == None`) or absent altogether.
fn github_ci_passed(checks: Option<&[GithubCheck]>) -> bool {
    crate::github_ci::classify(
        checks
            .unwrap_or_default()
            .iter()
            .map(|check| check.conclusion.as_deref()),
    ) == crate::github_ci::GithubCiStatus::Passed
}

#[derive(Debug, Deserialize)]
struct GitlabMr {
    title: String,
    #[serde(default)]
    description: Option<String>,
    source_branch: String,
    #[serde(default)]
    web_url: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    iid: Option<serde_json::Value>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    sha: Option<String>,
    #[serde(default)]
    detailed_merge_status: Option<String>,
    #[serde(default)]
    merge_status: Option<String>,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    head_pipeline: Option<GitlabPipeline>,
}

#[derive(Debug, Deserialize)]
struct GitlabPipeline {
    #[serde(default)]
    status: Option<String>,
}

fn gitlab_mrs(
    profile: &crate::config::Profile,
    scope: MrFetchScope,
    filter_gah_branches: bool,
) -> Result<Vec<SyncMr>> {
    let mut synced = Vec::new();
    const PAGE_SIZE: usize = 100;
    let project_id = gitlab_project_id(profile)?;
    let state = match scope {
        MrFetchScope::Active => "opened",
        MrFetchScope::FullHistory => "all",
    };
    let mut page = 1;
    loop {
        let response = gitlab_api(
            profile,
            &format!("projects/{project_id}/merge_requests"),
            "GET",
            &[
                ("scope", "all"),
                ("state", state),
                ("per_page", &PAGE_SIZE.to_string()),
                ("page", &page.to_string()),
            ],
        )?;
        let mrs: Vec<GitlabMr> = serde_json::from_value(response)?;
        let count = mrs.len();
        for mr in mrs
            .into_iter()
            .filter(|mr| !filter_gah_branches || mr.source_branch.starts_with("gah/"))
        {
            let iid = mr
                .iid
                .as_ref()
                .map(|value| value.to_string().trim_matches('"').to_string());
            let mut pipeline_status = mr.head_pipeline.and_then(|pipeline| pipeline.status);
            let mut awaits_ci = true;
            if pipeline_status.is_none() && mr.state.as_deref() == Some("opened") {
                if let Some(iid) = iid.as_deref() {
                    pipeline_status = gitlab_latest_pipeline_status(profile, iid)?;
                }
                awaits_ci = pipeline_status.is_some()
                    || touched_within(mr.updated_at.as_deref(), NO_PIPELINE_GRACE);
            }
            synced.push(SyncMr {
                work_id: extract_work_id_from_title(&mr.title),
                title: mr.title,
                body: mr.description,
                branch: mr.source_branch,
                labels: mr.labels,
                url: mr.web_url,
                id: iid,
                state: mr.state,
                draft: mr.draft,
                source_sha: mr.sha,
                merge_commit_sha: None,
                merge_status: mr.detailed_merge_status.or(mr.merge_status),
                merged: mr.merged_at.is_some(),
                updated_at: mr.updated_at,
                merged_at: mr.merged_at,
                ci_failed: gitlab_ci_failed(pipeline_status.as_deref()),
                ci_passed: gitlab_ci_passed(pipeline_status.as_deref()),
                ci_pending: awaits_ci && gitlab_ci_pending(pipeline_status.as_deref()),
            });
        }
        if count < PAGE_SIZE {
            break;
        }
        page += 1;
    }
    Ok(synced)
}

fn gitlab_latest_pipeline_status(
    profile: &crate::config::Profile,
    iid: &str,
) -> Result<Option<String>> {
    let project_id = gitlab_project_id(profile)?;
    let endpoint = format!("projects/{project_id}/merge_requests/{iid}/pipelines");
    let response = crate::provider::gitlab_api(profile, &endpoint, "GET", &[("per_page", "1")])?;
    let pipelines = response.as_array().ok_or_else(|| {
        anyhow::anyhow!(
            "GitLab pipeline lookup returned a non-list response for MR !{}: {}",
            iid,
            crate::redact::redact(&response.to_string())
        )
    })?;
    Ok(pipelines
        .first()
        .and_then(|pipeline| pipeline.get("status"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string))
}

/// How long an open MR with no pipeline is still taken to be waiting for
/// one. GitLab creates a pipeline within moments of a push, so an MR left
/// alone this long without one has no CI coming: its project runs none, or
/// runs none for this branch. Waiting longer would never end.
const NO_PIPELINE_GRACE: time::Duration = time::Duration::minutes(10);

/// Whether `updated_at` (RFC 3339) is within `window` of now. A missing or
/// unreadable time counts as recent, so the caller keeps waiting.
fn touched_within(updated_at: Option<&str>, window: time::Duration) -> bool {
    updated_at
        .and_then(|at| {
            time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).ok()
        })
        .is_none_or(|at| crate::clock::now_utc() - at < window)
}

fn gitlab_ci_failed(pipeline_status: Option<&str>) -> bool {
    matches!(pipeline_status, Some("failed") | Some("canceled"))
}

fn gitlab_ci_passed(pipeline_status: Option<&str>) -> bool {
    pipeline_status == Some("success")
}

/// True when the GitLab pipeline status is *non-terminal / unknown* -- a
/// pipeline still `running`/`pending`, or no pipeline reported at all
/// (`head_pipeline` absent). Distinct from both `gitlab_ci_passed` and
/// `gitlab_ci_failed` so the controller can surface a visible "wait and
/// re-check" action instead of letting an MR silently no-op forever
/// (issue #156). `skipped`/`manual`/`created` are treated as pending too:
/// they are not a conclusive pass or failure and must not be merged.
fn gitlab_ci_pending(pipeline_status: Option<&str>) -> bool {
    matches!(
        pipeline_status,
        None | Some("running")
            | Some("pending")
            | Some("created")
            | Some("skipped")
            | Some("manual")
    )
}

/// Count only post-review repairs from the active source/metadata generation.
/// Internal backend retries within one dispatch still count as one repair
/// cycle, and `clear_attempts` tombstones reset the append-only history.
pub fn count_current_fix_attempts_for_mrs(
    entries: &[crate::ledger::LedgerEntry],
    profile: &str,
    repo_id: &str,
    merge_requests: &[SyncMrJson],
) -> std::collections::HashMap<String, usize> {
    let generations: std::collections::HashMap<&str, &str> = merge_requests
        .iter()
        .filter_map(|mr| Some((mr.branch.as_str(), mr.review_generation.as_deref()?)))
        .collect();
    count_branch_attempts(entries, Some((profile, repo_id)), |entry| {
        usize::from(
            JobKind::parse(&entry.mode) == Ok(JobKind::Fix)
                && entry.dispatch_reason.as_deref() == Some("post_review_repair")
                && entry.review_contract_version
                    == Some(crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION)
                && entry
                    .review_generation
                    .as_deref()
                    .is_some_and(|generation| {
                        entry
                            .branch
                            .as_deref()
                            .and_then(|branch| generations.get(branch).copied())
                            == Some(generation)
                    }),
        )
    })
}

/// Used to cap auto-merge retries (TICKET-127): a merge that fails
/// (conflicts, unresolved discussions, external checks) would otherwise be
/// re-attempted every loop iteration forever.
pub fn count_merge_attempts_per_branch_for_scope(
    entries: &[crate::ledger::LedgerEntry],
    profile: &str,
    repo_id: &str,
) -> std::collections::HashMap<String, usize> {
    count_branch_attempts(entries, Some((profile, repo_id)), |entry| {
        if entry.mode == "merge" {
            entry.attempts_started.unwrap_or(0) as usize
        } else {
            0
        }
    })
}

fn count_branch_attempts(
    entries: &[crate::ledger::LedgerEntry],
    scope: Option<(&str, &str)>,
    attempt_weight: impl Fn(&crate::ledger::LedgerEntry) -> usize,
) -> std::collections::HashMap<String, usize> {
    use std::collections::{HashMap, HashSet};

    let in_scope = |entry: &crate::ledger::LedgerEntry| {
        scope.is_none_or(|(profile, repo_id)| entry.profile == profile && entry.repo_id == repo_id)
    };
    let mut counts = HashMap::new();
    let mut branches_by_work: HashMap<(String, String, String), HashSet<String>> = HashMap::new();

    for entry in entries.iter().filter(|entry| in_scope(entry)) {
        if entry.mode == "clear_attempts" {
            let Some(work_id) = entry.work_id.as_deref() else {
                continue;
            };
            let aliases = crate::ledger::work_id_aliases(work_id);
            let branches_to_reset: HashSet<String> = branches_by_work
                .iter()
                .filter(|((profile, repo_id, known_work_id), _)| {
                    profile == &entry.profile
                        && repo_id == &entry.repo_id
                        && aliases.iter().any(|alias| alias == known_work_id)
                })
                .flat_map(|(_, branches)| branches.iter().cloned())
                .collect();
            for branch in branches_to_reset {
                counts.remove(&branch);
            }
            continue;
        }

        // Reviews and other records often carry the authoritative issue ID
        // even when an older FixMr record used its branch as a synthetic ID.
        // Retain every attributable branch/work pairing so an operator reset
        // can also clear those historical repair counters.
        if let (Some(branch), Some(work_id)) = (&entry.branch, &entry.work_id) {
            branches_by_work
                .entry((
                    entry.profile.clone(),
                    entry.repo_id.clone(),
                    work_id.clone(),
                ))
                .or_default()
                .insert(branch.clone());
        }

        let weight = attempt_weight(entry);
        if weight == 0 {
            continue;
        }
        let Some(branch) = entry.branch.as_ref() else {
            continue;
        };
        *counts.entry(branch.clone()).or_insert(0) += weight;
    }

    counts
}

#[cfg(test)]
mod tests;
