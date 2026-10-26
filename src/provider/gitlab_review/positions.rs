use super::*;

/// Anchor only to a line actually present on the requested side of this diff version.
pub(super) fn discussion(
    version: &Value,
    diffs: &[Value],
    finding: &ActionableReviewFinding,
) -> Result<Value> {
    let line = finding.line.as_deref().context("finding has no line")?;
    let (old_side, line) = match line.strip_prefix("old:") {
        Some(line) => (true, line),
        None => (false, line.strip_prefix("new:").unwrap_or(line)),
    };
    let line: u32 = line.parse().context("finding line must be a number")?;
    let diff = diffs
        .iter()
        .find(|d| d[if old_side { "old_path" } else { "new_path" }].as_str() == Some(&finding.file))
        .context("finding file missing from diff")?;
    let (old, new) = diff_line(
        diff["diff"].as_str().context("diff missing")?,
        old_side,
        line,
    )
    .context("finding line outside diff")?;
    let mut position =
        json!({"position_type":"text", "old_path":diff["old_path"], "new_path":diff["new_path"]});
    for (field, source) in [
        ("base_sha", "base_commit_sha"),
        ("start_sha", "start_commit_sha"),
        ("head_sha", "head_commit_sha"),
    ] {
        let sha = version[source]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("diff SHA missing")?;
        position[field] = json!(sha);
    }
    if let Some(old) = old {
        position["old_line"] = json!(old);
    }
    if let Some(new) = new {
        position["new_line"] = json!(new);
    }
    // GitLab cannot apply a replacement to a deleted line.
    if old_side && finding.summary.contains("```suggestion") {
        bail!("suggestion requires a new-side line");
    }
    Ok(json!({"body": publication_body(&finding.summary), "position":position}))
}

fn diff_line(patch: &str, old_side: bool, wanted: u32) -> Option<(Option<u32>, Option<u32>)> {
    let mut old = 0;
    let mut new = 0;
    let mut in_hunk = false;
    for line in patch.lines() {
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
            in_hunk = true;
        } else if in_hunk {
            let pair = match line.as_bytes().first() {
                Some(b'+') => (None, Some(new)),
                Some(b'-') => (Some(old), None),
                Some(b' ') => (Some(old), Some(new)),
                _ => continue,
            };
            if (if old_side { pair.0 } else { pair.1 }) == Some(wanted) && wanted > 0 {
                return Some(pair);
            }
            if pair.0.is_some() {
                old += 1;
            }
            if pair.1.is_some() {
                new += 1;
            }
        }
    }
    None
}
