//! Review receipts stop repeat reviewer calls without granting merge or repair authority.
use crate::config::Profile;
use crate::ledger::LedgerEntry;
use crate::provider_kind::ProviderKind;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const MARKER: &str = "<!-- gah-review-head-v1 ";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    head: String,
    backend: String,
    model: Option<String>,
}

/// Append an immutable head and exact reviewer route to a published verdict.
pub(crate) fn stamp(body: &mut String, entry: &LedgerEntry) -> Result<()> {
    if let Some(head) = &entry.review_source_sha {
        let receipt = Receipt {
            head: head.clone(),
            backend: entry.effective_backend.clone(),
            model: entry.effective_model.clone(),
        };
        body.push_str(&format!(
            "\n{MARKER}{} -->\n",
            serde_json::to_string(&receipt)?
        ));
    }
    Ok(())
}

/// Read the immutable head marker; malformed markers cannot become legacy verdicts.
pub(crate) fn comment_head(body: &str) -> Result<Option<String>> {
    let mut head = None;
    for line in body.lines().filter(|line| line.starts_with(MARKER)) {
        let receipt: Receipt = serde_json::from_str(
            line.strip_prefix(MARKER)
                .and_then(|line| line.strip_suffix(" -->"))
                .ok_or_else(|| anyhow::anyhow!("malformed review head receipt"))?,
        )?;
        anyhow::ensure!(
            head.is_none() && !receipt.head.trim().is_empty() && !receipt.backend.trim().is_empty(),
            "invalid or duplicate review head receipt"
        );
        head = Some(receipt.head);
    }
    Ok(head)
}

fn has_receipt(body: &str, head: &str) -> bool {
    let verdict = body.lines().next().and_then(|line| {
        line.strip_prefix("GAH review verdict: `")?
            .strip_suffix('`')
    });
    if !matches!(
        verdict,
        Some("APPROVE" | "NEEDS_FIX" | "REJECT" | "HUMAN_REVIEW")
    ) {
        return false;
    }
    comment_head(body).ok().flatten().as_deref() == Some(head)
}

/// A completed opinion is a head-level stop, even when metadata or routing changes.
/// Invalid output and interrupted calls remain eligible for bounded rerouting.
pub(crate) fn locally_reviewed(
    entries: &[LedgerEntry],
    profile_name: &str,
    profile: &Profile,
    branch: &str,
    mr_url: Option<&str>,
    head: &str,
) -> bool {
    entries.iter().any(|entry| {
        entry.profile == profile_name
            && entry.repo_id == profile.repo_id
            && entry.mode == "review"
            && entry.failure_class.is_none()
            && entry.branch.as_deref() == Some(branch)
            && (mr_url.is_none() || entry.mr_url.as_deref() == mr_url)
            && entry.review_source_sha.as_deref() == Some(head)
            && (entry.validation_result.as_deref() == Some("skipped_duplicate_review")
                || matches!(
                    entry.review_verdict.as_deref(),
                    Some("APPROVE" | "NEEDS_FIX" | "REJECT" | "HUMAN_REVIEW")
                ))
    })
}

#[derive(Deserialize)]
struct Comment {
    body: String,
}

/// Read all comment pages so a review survives node changes and busy PR threads.
/// Receipts only suppress reviewer calls; their contents never authorize a merge.
pub(crate) fn remotely_reviewed(profile: &Profile, number: &str, head: &str) -> Result<bool> {
    anyhow::ensure!(
        !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()),
        "invalid PR/MR number"
    );
    let provider = ProviderKind::parse(&profile.provider)?;
    let endpoint = match provider {
        ProviderKind::Github => format!("repos/{}/issues/{number}/comments", profile.repo),
        ProviderKind::Gitlab => format!(
            "projects/{}/merge_requests/{number}/notes",
            crate::provider::gitlab_project_id(profile)?
        ),
    };
    for page in 1.. {
        let endpoint = format!("{endpoint}?per_page=100&page={page}");
        let comments: Vec<Comment> = match provider {
            ProviderKind::Github => {
                let output = crate::provider::provider_command("gh")
                    .args(["api", "--method", "GET", &endpoint])
                    .output()
                    .context("reading review head receipts")?;
                anyhow::ensure!(
                    output.status.success(),
                    "reading review head receipts failed: {}",
                    crate::redact::redact(String::from_utf8_lossy(&output.stderr).trim())
                );
                serde_json::from_slice(&output.stdout).context("parsing review head receipts")?
            }
            ProviderKind::Gitlab => serde_json::from_value(crate::provider::gitlab_api(
                profile,
                &endpoint,
                "GET",
                &[],
            )?)?,
        };
        if comments
            .iter()
            .any(|comment| has_receipt(&comment.body, head))
        {
            return Ok(true);
        }
        if comments.len() < 100 {
            return Ok(false);
        }
    }
    unreachable!("comment pagination returns at the last page")
}

/// Populate only the repeat-review gate, keeping lifecycle authorization independent.
pub(crate) fn project(
    profile_name: &str,
    profile: &Profile,
    entries: &[LedgerEntry],
    row: &mut crate::sync::SyncMrJson,
) -> Result<()> {
    let Some(head) = row.source_sha.as_deref() else {
        return Ok(());
    };
    let local = locally_reviewed(
        entries,
        profile_name,
        profile,
        &row.branch,
        row.url.as_deref(),
        head,
    );
    let needs_review = row.classification == "NEEDS_REVIEW"
        || (matches!(row.classification.as_str(), "CI_FAILED" | "NEEDS_FIX")
            && !(row.review_generation_status.is_none()
                && matches!(row.review_verdict.as_deref(), Some("NEEDS_FIX" | "REJECT"))));
    let remote = if !local && needs_review {
        match row.id.as_deref() {
            Some(number) => remotely_reviewed(profile, number, head)?,
            None => false,
        }
    } else {
        false
    };
    if local || remote {
        row.last_reviewed_head = Some(head.to_string());
    }
    Ok(())
}

/// Whether this snapshot would repeat a completed review of the current head.
pub(crate) fn unchanged(row: &crate::sync::SyncMrJson) -> bool {
    row.source_sha.is_some() && row.source_sha == row.last_reviewed_head
}

#[cfg(test)]
mod tests;
