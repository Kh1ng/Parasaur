use super::context::ReviewTarget;
use crate::config::{GahConfig, Profile};
use crate::ledger::{self, LedgerEntry};
use anyhow::Result;

/// Stop before route selection or escalation when this head already has an opinion.
pub(in crate::dispatch) fn skip_reviewed_head(
    cfg: &GahConfig,
    profile: &Profile,
    target: &ReviewTarget,
    entry: &mut LedgerEntry,
) -> Result<bool> {
    if profile.publishing.allow_issue_comments {
        crate::policy::enforce_policy(profile, "review")?;
    }
    let Some(head) = target.source_sha.as_deref() else {
        return Ok(false);
    };
    let local = crate::review_head::locally_reviewed(
        &ledger::read_entries(cfg)?,
        &entry.profile,
        profile,
        &target.source_branch,
        target.mr_url.as_deref(),
        head,
    );
    let remote = if !local {
        match target.mr_id.as_deref() {
            Some(number) => crate::review_head::remotely_reviewed(profile, number, head)?,
            None => false,
        }
    } else {
        false
    };
    if local || remote {
        entry.validation_result = Some("skipped_duplicate_review".into());
        println!(
            "Skipping already reviewed head {head} on {}",
            target.source_branch
        );
        return Ok(true);
    }
    Ok(false)
}

/// Escalation belongs to the immutable head, independently of title/body edits.
pub(super) fn same_head(entry: &LedgerEntry, generation: Option<&str>) -> bool {
    let head = generation
        .and_then(|value| {
            value.strip_prefix(&format!(
                "review-v{}:",
                ledger::CURRENT_REVIEW_CONTRACT_VERSION
            ))
        })
        .and_then(|value| value.split_once(':'))
        .map(|(head, _)| head);
    head.is_some() && entry.review_source_sha.as_deref() == head
}
