//! All reviewers can veto a merge. Only trusted reviewers can supply repair imports.
use super::{github_pages, RestReview, RestUser};
use crate::config::Profile;
use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Approve,
    NeedsFix,
    Reject,
    HumanReview,
}

impl Verdict {
    fn parse(body: &str) -> Option<Self> {
        let value = body.lines().next()?.strip_prefix("GAH review verdict: ")?;
        let value = value
            .strip_prefix('`')
            .and_then(|v| v.strip_suffix('`'))
            .unwrap_or(value);
        match value {
            "APPROVE" => Some(Self::Approve),
            "NEEDS_FIX" => Some(Self::NeedsFix),
            "REJECT" => Some(Self::Reject),
            "HUMAN_REVIEW" => Some(Self::HumanReview),
            _ => None,
        }
    }

    fn blocks(self) -> bool {
        match self {
            Self::Approve => false,
            Self::NeedsFix | Self::Reject | Self::HumanReview => true,
        }
    }
}

#[derive(Deserialize)]
struct Comment {
    id: u64,
    user: Option<RestUser>,
    body: String,
    created_at: String,
    #[serde(default)]
    pull_request_review_id: Option<u64>,
}

#[derive(Deserialize)]
struct TimelineEvent {
    event: String,
    id: Option<u64>,
}

/// Use timeline position, not authored/committed dates: a push can contain old commits.
fn legacy_current(id: u64, review: bool, timeline: &[TimelineEvent]) -> bool {
    let last_push = timeline.iter().rposition(|event| {
        matches!(
            event.event.as_str(),
            "committed" | "head_ref_force_pushed" | "head_ref_restored"
        )
    });
    let Some(last_push) = last_push else {
        return true;
    };
    timeline.iter().enumerate().any(|(index, event)| {
        index > last_push
            && event.id == Some(id)
            && event.event == if review { "reviewed" } else { "commented" }
    })
}

struct Opinion {
    login: String,
    at: String,
    id: u64,
    verdict: Verdict,
}

fn comment_opinion(
    comment: Comment,
    head: &str,
    timeline: &[TimelineEvent],
) -> Result<Option<Opinion>> {
    let Some(verdict) = Verdict::parse(&comment.body) else {
        return Ok(None);
    };
    let applies = match crate::review_head::comment_head(&comment.body)? {
        Some(receipt) => receipt == head,
        None => legacy_current(
            comment.pull_request_review_id.unwrap_or(comment.id),
            comment.pull_request_review_id.is_some(),
            timeline,
        ),
    };
    Ok(comment.user.filter(|_| applies).map(|user| Opinion {
        login: user.login.to_ascii_lowercase(),
        at: comment.created_at,
        id: comment.id,
        verdict,
    }))
}

/// Return the current head's vetoes, retaining each login's newest opinion across
/// issue comments, inline comments, review bodies and formal GitHub reviews.
/// Approvals withdraw only that login's veto and never grant merge authority.
pub(crate) fn blockers(profile: &Profile, number: &str, head: &str) -> Result<Vec<String>> {
    anyhow::ensure!(
        !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()),
        "invalid PR number"
    );
    anyhow::ensure!(!head.is_empty(), "missing PR head for merge review gate");
    let root = format!("repos/{}", profile.repo);
    let mut comments: Vec<Comment> = github_pages(
        &format!("{root}/issues/{number}/comments"),
        "verdict comments",
    )?;
    comments.extend(github_pages::<Comment>(
        &format!("{root}/pulls/{number}/comments"),
        "inline verdict comments",
    )?);
    let mut reviews: Vec<RestReview> =
        github_pages(&format!("{root}/pulls/{number}/reviews"), "merge reviews")?;
    reviews.sort_by(|a, b| (&a.submitted_at, a.id).cmp(&(&b.submitted_at, b.id)));
    let mut decisive = BTreeMap::new();
    for review in reviews {
        if matches!(
            review.state.as_str(),
            "CHANGES_REQUESTED" | "APPROVED" | "DISMISSED"
        ) {
            if let Some(user) = review
                .user
                .as_ref()
                .filter(|_| review.commit_id.as_deref() == Some(head))
            {
                decisive.insert(user.login.to_ascii_lowercase(), review);
            }
        } else if review.state == "COMMENTED" {
            comments.push(Comment {
                id: review.id,
                user: review.user,
                body: review.body.unwrap_or_default(),
                created_at: review.submitted_at.unwrap_or_default(),
                pull_request_review_id: Some(review.id),
            });
        }
    }
    let mut opinions = Vec::new();
    for (login, review) in decisive {
        let verdict = match review.state.as_str() {
            "CHANGES_REQUESTED" => Verdict::NeedsFix,
            "APPROVED" => Verdict::Approve,
            _ => continue, // Dismissal withdraws the formal request, never a comment veto.
        };
        opinions.push(Opinion {
            login,
            at: review.submitted_at.unwrap_or_default(),
            id: review.id,
            verdict,
        });
    }
    let needs_timeline = comments.iter().any(|comment| {
        Verdict::parse(&comment.body).is_some()
            && matches!(crate::review_head::comment_head(&comment.body), Ok(None))
    });
    let timeline = if needs_timeline {
        github_pages(
            &format!("{root}/issues/{number}/timeline"),
            "legacy verdict head history",
        )?
    } else {
        Vec::new()
    };
    for comment in comments {
        if let Some(opinion) = comment_opinion(comment, head, &timeline)? {
            opinions.push(opinion);
        }
    }
    opinions.sort_by(|a, b| (&a.at, a.id).cmp(&(&b.at, b.id)));
    let mut latest = BTreeMap::new();
    for opinion in opinions {
        latest.insert(opinion.login, opinion.verdict);
    }
    Ok(latest
        .into_iter()
        .filter_map(|(login, verdict)| verdict.blocks().then_some(login))
        .collect())
}

/// Project only a merge veto, independent of local review and repair authority.
pub(crate) fn project(profile: &Profile, row: &mut crate::sync::SyncMrJson) -> Result<()> {
    if profile.provider != "github" || row.classification != "READY_FOR_HUMAN" {
        return Ok(());
    }
    let number = row
        .id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("merge candidate missing PR number"))?;
    let head = row
        .source_sha
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("merge candidate missing PR head"))?;
    let reviewers = blockers(profile, number, head)?;
    row.merge_review_block_reason = (!reviewers.is_empty())
        .then(|| format!("current-head review veto from {}", reviewers.join(", ")));
    Ok(())
}

#[cfg(test)]
mod tests;
