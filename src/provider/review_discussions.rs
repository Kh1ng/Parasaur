//! GitLab review conversations. These calls deliberately use the configured bot
//! PAT rather than the interactive glab login of the requesting user.
use super::*;
use crate::models::ActionableReviewFinding;
use serde::{Deserialize, Serialize};

#[cfg(test)]
thread_local! {
    pub(super) static TEST_BOT_TOKEN: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn bot_token(profile: &Profile) -> String {
    #[cfg(test)]
    if let Some(token) = TEST_BOT_TOKEN.with(|value| value.borrow().clone()) {
        return token;
    }
    profile.pat()
}

fn bot_api(
    profile: &Profile,
    endpoint: &str,
    method: &str,
    fields: &[(&str, &str)],
) -> Result<serde_json::Value> {
    let token = bot_token(profile);
    anyhow::ensure!(
        !token.trim().is_empty(),
        "GitLab reviews require the bot GITLAB_PAT2 or GITLAB_PAT"
    );
    let host = gitlab_hostname(
        profile
            .provider_api_base
            .as_deref()
            .context("missing GitLab API base")?,
    )?;
    let mut command = provider_command("glab");
    command
        .env("GITLAB_TOKEN", &token)
        .env("GITLAB_ACCESS_TOKEN", &token);
    command.args(["api", endpoint, "--hostname", host, "--method", method]);
    for (key, value) in fields {
        command.args(["--raw-field", &format!("{key}={value}")]);
    }
    let out = command.output().context("GitLab bot review request")?;
    anyhow::ensure!(
        out.status.success(),
        "GitLab bot review request failed: {}",
        redacted_provider_output(&out)
    );
    if out.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(serde_json::Value::Null);
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

fn root(profile: &Profile, branch: &str) -> Result<String> {
    let project = gitlab_project_id(profile)?;
    let matches = bot_api(
        profile,
        &format!("projects/{project}/merge_requests"),
        "GET",
        &[("source_branch", branch), ("state", "opened")],
    )?;
    let matches = matches.as_array().context("expected GitLab MR array")?;
    anyhow::ensure!(
        matches.len() == 1,
        "expected exactly one open GitLab MR for branch {branch}"
    );
    let iid = matches[0]["iid"]
        .as_u64()
        .context("missing GitLab MR iid")?;
    Ok(format!("projects/{project}/merge_requests/{iid}"))
}

/// Keep the provider-neutral summary and publish positionable findings inline.
/// Findings without a new-side line remain visible in the summary.
pub fn post_review_findings(
    profile: &Profile,
    branch: &str,
    body: &str,
    labels: &[&str],
    findings: &[ActionableReviewFinding],
    expected_head: Option<&str>,
) -> Result<()> {
    if profile.provider != "gitlab" {
        return post_review_comment(profile, branch, body, labels);
    }
    anyhow::ensure!(
        profile.delivery_mode != crate::config::DeliveryMode::Handoff,
        "handoff mode forbids review publication"
    );
    let root = root(profile, branch)?;
    if findings.iter().any(|f| new_line(f).is_some()) {
        let mr = bot_api(profile, &root, "GET", &[])?;
        let refs = &mr["diff_refs"];
        let sha = |name: &str| -> Result<String> {
            Ok(refs[name]
                .as_str()
                .filter(|s| !s.is_empty())
                .with_context(|| format!("missing GitLab diff_refs.{name}"))?
                .to_owned())
        };
        let base = sha("base_sha")?;
        let start = sha("start_sha")?;
        let head = sha("head_sha")?;
        anyhow::ensure!(
            expected_head.is_none_or(|expected| expected == head),
            "GitLab MR head changed since review; review again before publishing suggestions"
        );
        let mut diffs = Vec::new();
        for page in 1.. {
            let response = bot_api(
                profile,
                &format!("{root}/diffs?per_page=100&page={page}"),
                "GET",
                &[],
            )?;
            let page = response.as_array().context("expected GitLab diffs array")?;
            diffs.extend(page.iter().cloned());
            if page.len() < 100 {
                break;
            }
        }
        for finding in findings {
            let Some(line) = new_line(finding) else {
                continue;
            };
            let Some(diff) = diffs
                .iter()
                .find(|diff| diff["new_path"].as_str() == Some(&finding.file))
            else {
                continue;
            };
            let Some(old_line) = diff["diff"]
                .as_str()
                .and_then(|diff| old_line_at(diff, line))
            else {
                continue;
            };
            let old_path = diff["old_path"].as_str().context("missing diff old_path")?;
            let mut body = finding.summary.clone();
            if let Some(replacement) = &finding.suggestion {
                anyhow::ensure!(
                    !replacement.contains("```"),
                    "suggestion must not contain Markdown fences"
                );
                body.push_str(&format!("\n\n```suggestion\n{replacement}\n```"));
            }
            let body = publication_body(&body);
            let line = line.to_string();
            let old_line = old_line.map(|line| line.to_string());
            let mut fields = vec![
                ("body", body.as_str()),
                ("position[position_type]", "text"),
                ("position[base_sha]", base.as_str()),
                ("position[start_sha]", start.as_str()),
                ("position[head_sha]", head.as_str()),
                ("position[old_path]", old_path),
                ("position[new_path]", finding.file.as_str()),
                ("position[new_line]", line.as_str()),
            ];
            if let Some(old_line) = &old_line {
                fields.push(("position[old_line]", old_line));
            }
            bot_api(profile, &format!("{root}/discussions"), "POST", &fields)?;
        }
    }
    bot_api(
        profile,
        &format!("{root}/notes"),
        "POST",
        &[("body", &publication_body(body))],
    )?;
    let remove_labels = GAH_REVIEW_STATE_LABELS
        .iter()
        .copied()
        .filter(|candidate| !labels.contains(candidate))
        .collect::<Vec<_>>()
        .join(",");
    bot_api(
        profile,
        &root,
        "PUT",
        &[
            ("add_labels", &labels.join(",")),
            ("remove_labels", &remove_labels),
        ],
    )?;
    Ok(())
}

// Added lines require only new_line; unchanged context needs both coordinates.
// Lines outside returned hunks (including truncated diffs) stay in the summary.
fn old_line_at(diff: &str, requested: u64) -> Option<Option<u64>> {
    let (mut old, mut new) = (0, 0);
    for line in diff.lines() {
        if line.starts_with("@@ ") {
            let mut parts = line.split_whitespace();
            parts.next()?;
            old = parts
                .next()?
                .strip_prefix('-')?
                .split(',')
                .next()?
                .parse()
                .ok()?;
            new = parts
                .next()?
                .strip_prefix('+')?
                .split(',')
                .next()?
                .parse()
                .ok()?;
        } else if line.starts_with('+') {
            if new == requested {
                return Some(None);
            }
            new += 1;
        } else if line.starts_with('-') {
            old += 1;
        } else if line.starts_with(' ') {
            if new == requested {
                return Some(Some(old));
            }
            old += 1;
            new += 1;
        }
    }
    None
}

fn new_line(finding: &ActionableReviewFinding) -> Option<u64> {
    finding
        .line
        .as_deref()?
        .parse()
        .ok()
        .filter(|line| *line > 0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDiscussion {
    pub id: String,
    pub file: Option<String>,
    pub position: Option<serde_json::Value>,
    pub notes: Vec<String>,
}

pub fn unresolved_review_discussions(
    profile: &Profile,
    branch: &str,
) -> Result<Vec<ReviewDiscussion>> {
    if profile.provider != "gitlab" {
        return Ok(Vec::new());
    }
    let root = root(profile, branch)?;
    let mut result = Vec::new();
    for page in 1.. {
        let value = bot_api(
            profile,
            &format!("{root}/discussions?per_page=100&page={page}"),
            "GET",
            &[],
        )?;
        let discussions = value
            .as_array()
            .context("expected GitLab discussions array")?;
        for discussion in discussions {
            let notes = discussion["notes"]
                .as_array()
                .context("missing discussion notes")?;
            if !notes
                .iter()
                .any(|n| n["resolvable"] == true && n["resolved"] == false)
            {
                continue;
            }
            result.push(ReviewDiscussion {
                id: discussion["id"]
                    .as_str()
                    .context("missing discussion id")?
                    .to_owned(),
                file: notes
                    .iter()
                    .find_map(|n| n["position"]["new_path"].as_str().map(str::to_owned)),
                position: notes
                    .iter()
                    .find_map(|n| n.get("position").filter(|p| p.is_object()).cloned()),
                notes: notes
                    .iter()
                    .filter_map(|n| n["body"].as_str().map(str::to_owned))
                    .collect(),
            });
        }
        if discussions.len() < 100 {
            break;
        }
    }
    Ok(result)
}

/// Only explicit, nonempty reports for snapshotted threads on changed files
/// qualify. Callers must invoke this only after validation and push succeed.
pub fn resolve_addressed_discussions(
    profile: &Profile,
    branch: &str,
    discussions: &[ReviewDiscussion],
    summary: &str,
    changed_files: &[String],
) -> Result<()> {
    anyhow::ensure!(
        profile.delivery_mode != crate::config::DeliveryMode::Handoff,
        "handoff mode forbids discussion resolution"
    );
    if discussions.is_empty() {
        return Ok(());
    }
    let root = root(profile, branch)?;
    for discussion in discussions {
        if changed_files.is_empty()
            || discussion
                .file
                .as_ref()
                .is_some_and(|file| !changed_files.contains(file))
        {
            continue;
        }
        let report = summary
            .lines()
            .filter_map(|line| line.strip_prefix("GAH_ADDRESSED_DISCUSSION: "))
            .filter_map(|line| line.split_once(" | "))
            .find(|(id, reply)| *id == discussion.id && !reply.trim().is_empty());
        let Some((_, reply)) = report else { continue };
        // Reply before resolving; failure leaves the thread open.
        let endpoint = format!("{root}/discussions/{}", discussion.id);
        let current = bot_api(profile, &endpoint, "GET", &[])?;
        let notes = current["notes"]
            .as_array()
            .context("missing discussion notes")?;
        let bodies: Vec<String> = notes
            .iter()
            .filter_map(|n| n["body"].as_str().map(str::to_owned))
            .collect();
        if bodies != discussion.notes
            || !notes
                .iter()
                .any(|n| n["resolvable"] == true && n["resolved"] == false)
        {
            continue;
        }
        bot_api(
            profile,
            &format!("{endpoint}/notes"),
            "POST",
            &[("body", &publication_body(reply))],
        )?;
        bot_api(profile, &endpoint, "PUT", &[("resolved", "true")])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::old_line_at;

    #[test]
    fn diff_positions_distinguish_added_context_and_missing_lines() {
        let diff =
            "@@ -10,3 +10,3 @@\n context\n-removed\n+added\n context\n@@ -50,1 +50,1 @@\n tail";
        assert_eq!(old_line_at(diff, 10), Some(Some(10)));
        assert_eq!(old_line_at(diff, 11), Some(None));
        assert_eq!(old_line_at(diff, 12), Some(Some(12)));
        assert_eq!(old_line_at(diff, 50), Some(Some(50)));
        assert_eq!(old_line_at(diff, 30), None);
    }
}
