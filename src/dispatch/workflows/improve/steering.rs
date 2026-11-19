//! Recover the trusted GitLab issue mentions queued while a managed worker was interrupted.
use crate::config::{GahConfig, Profile};
use crate::dispatch::{issues::gitlab_note_author_is_trusted, prompts::indent_untrusted_text};
use crate::job_kind::{JobFamily, JobKind};
use crate::ledger::{self, LedgerEntry};
use crate::provider::{gitlab_api, gitlab_project_id};
use anyhow::{Context, Result};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

// Require a directly addressed note. Mentions inside quotations or code are evidence, never steering.
static MENTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^@parasaur(?:$|[^a-z0-9_-])").unwrap());
const MAX_NOTE_BYTES: usize = 32_768;
const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 100;

pub(super) fn append(
    cfg: &GahConfig,
    profile: &Profile,
    current: &LedgerEntry,
    restored: Option<&String>,
    worktree: &Path,
    prompt: &mut String,
) -> Result<()> {
    let Some(checkpoint) = restored else {
        return Ok(());
    };
    let notes = queued_notes(cfg, profile, current, checkpoint, worktree)
        .inspect_err(|_| crate::worktree::cleanup(worktree, Path::new(&profile.local_path)))?;
    if let Some(notes) = notes {
        prompt.push_str(&notes);
    }
    Ok(())
}

fn queued_notes(
    cfg: &GahConfig,
    profile: &Profile,
    current: &LedgerEntry,
    checkpoint_branch: &str,
    worktree: &Path,
) -> Result<Option<String>> {
    if !profile.provider.eq_ignore_ascii_case("gitlab") {
        return Ok(None);
    }
    let Some(work_id) = current.work_id.as_deref() else {
        return Ok(None);
    };
    let entries = ledger::entries_for_work_id(cfg, work_id)?;
    let latest = entries.iter().rev().find(|entry| {
        entry.profile == current.profile
            && entry.repo_id == profile.repo_id
            && !ledger::is_entry_stale(entry)
            && (entry.mode == "clear_attempts"
                || (!ledger::gates::launched_no_backend(entry)
                    && JobKind::parse(&entry.mode)
                        .is_ok_and(|kind| kind.family() == JobFamily::ImproveLike)))
    });
    let Some(entry) = latest.filter(|e| {
        e.provider.eq_ignore_ascii_case(&profile.provider)
            && e.repo == profile.repo
            && e.validation_result.as_deref() == Some("cancelled_shutdown")
            && e.source_issue_number == current.source_issue_number
            && e.resumable_checkpoint_branch.as_deref() == Some(checkpoint_branch)
    }) else {
        return Ok(None);
    };
    let restored_sha = crate::worktree::git(&["rev-parse", "HEAD"], worktree)?;
    if entry.resumable_checkpoint_sha.as_deref() != Some(restored_sha.as_str()) {
        return Ok(None);
    }
    let Some(iid) = entry
        .source_issue_number
        .as_ref()
        .and_then(|s| s.parse::<u64>().ok())
    else {
        return Ok(None);
    };
    let since = OffsetDateTime::parse(&entry.timestamp, &Rfc3339)
        .context("invalid interrupted dispatch time")?;
    let project = gitlab_project_id(profile)?;
    let endpoint = format!("projects/{project}/issues/{iid}/notes");
    let bot = std::env::var("PARASAUR_GITLAB_BOT_USERNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "parasaur".into());
    let mut notes = BTreeMap::new();
    let mut bytes = 0;
    let mut more_notes = false;
    'pages: for page in 1..=MAX_PAGES {
        let result = gitlab_api(
            profile,
            &endpoint,
            "GET",
            &[
                ("order_by", "created_at"),
                ("sort", "desc"),
                ("per_page", &PAGE_SIZE.to_string()),
                ("page", &page.to_string()),
            ],
        )?;
        let rows = result
            .as_array()
            .context("invalid GitLab issue notes response")?;
        let mut older = false;
        for note in rows {
            let Some(created) = note["created_at"]
                .as_str()
                .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
            else {
                continue;
            };
            if created < since {
                older = true;
                continue;
            }
            let (Some(id), Some(body)) = (note["id"].as_u64(), note["body"].as_str()) else {
                continue;
            };
            if note["system"] != false
                || body.len() > MAX_NOTE_BYTES
                || !MENTION.is_match(body.trim_start_matches(['\r', '\n']))
                || note["author"]["username"]
                    .as_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&bot))
                || !gitlab_note_author_is_trusted(profile, note)
            {
                continue;
            }
            if notes.contains_key(&(created, id)) {
                continue;
            }
            // Keep the newest full notes within the GitLab instruction size.
            if bytes + body.len() > MAX_NOTE_BYTES {
                more_notes = true;
                break 'pages;
            }
            bytes += body.len();
            notes.insert((created, id), indent_untrusted_text(body));
        }
        if older || rows.len() < PAGE_SIZE {
            break;
        }
        anyhow::ensure!(
            page < MAX_PAGES,
            "GitLab issue notes exceeded {MAX_PAGES} pages within the interrupted dispatch"
        );
    }
    if notes.is_empty() {
        return Ok(None);
    }
    let mut context = "\n\n## Queued GitLab issue instructions\n\nIssue notes from trusted authors, subject to this job's scope and worker policy:\n".to_owned();
    if more_notes {
        context.push_str("Older queued mentions are available on this issue.\n");
    }
    for ((_, id), body) in notes {
        context.push_str(&format!("\nNote {id}:\n{body}\n"));
    }
    Ok(Some(context))
}
