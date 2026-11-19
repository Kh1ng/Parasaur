use super::IssueBudgetStatus;
use crate::ledger;
use serde::Serialize;

#[derive(Default, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct WorkWaypointEvidence {
    pub first_dispatch_at: Option<String>,
    /// Latest dispatch was interrupted on shutdown after preserving work.
    #[serde(skip)]
    pub(crate) interrupted_checkpoint: bool,
    pub first_commit_at: Option<String>,
    pub first_validation_at: Option<String>,
    pub first_pull_request_at: Option<String>,
    /// Per-issue budget usage and remaining allowance, present only when
    /// the profile's `routing.issue_budget` limits an axis and the issue
    /// has recorded usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_budget: Option<IssueBudgetStatus>,
}

pub(super) fn project(
    entries_by_work_id: &ledger::LedgerEntriesByWorkId,
    profile: &str,
    repo_id: &str,
) -> std::collections::BTreeMap<String, WorkWaypointEvidence> {
    entries_by_work_id
        .iter()
        .filter_map(|(work_id, entries)| {
            let mut evidence = WorkWaypointEvidence::default();
            for entry in entries {
                if entry.profile != profile
                    || entry.repo_id != repo_id
                    || ledger::is_entry_stale(entry)
                {
                    continue;
                }
                if entry.mode == "clear_attempts" {
                    evidence = WorkWaypointEvidence::default();
                    continue;
                }
                let is_dispatch = entry.dispatch_reason.is_some()
                    || entry.attempts_started.unwrap_or_default() > 0
                    || entry.backend_exit_code.is_some()
                    || entry.commit_attempted
                    || entry.validation_result.is_some();
                if is_dispatch {
                    evidence.interrupted_checkpoint = entry.validation_result.as_deref()
                        == Some("cancelled_shutdown")
                        && entry.resumable_checkpoint_branch.is_some();
                }
                if is_dispatch && evidence.first_dispatch_at.is_none() {
                    evidence.first_dispatch_at = Some(entry.timestamp.clone());
                }
                if entry.commit_created && evidence.first_commit_at.is_none() {
                    evidence.first_commit_at = Some(entry.timestamp.clone());
                }
                if entry.validation_result.as_deref() == Some("passed")
                    && evidence.first_validation_at.is_none()
                {
                    evidence.first_validation_at = Some(entry.timestamp.clone());
                }
                if entry.mr_created && evidence.first_pull_request_at.is_none() {
                    evidence.first_pull_request_at = Some(entry.timestamp.clone());
                }
            }
            (evidence != WorkWaypointEvidence::default()).then(|| (work_id.clone(), evidence))
        })
        .collect()
}
