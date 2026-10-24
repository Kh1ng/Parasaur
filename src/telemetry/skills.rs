//! Skill grouping retains per-attempt attribution and stable export records.
use crate::ledger::{skills, LedgerEntry};

pub(super) fn project_entries(entries: &[LedgerEntry], include_retried: bool) -> Vec<LedgerEntry> {
    let mut entries = entries.to_vec();
    if !include_retried {
        for entry in &mut entries {
            if let Some(last) = entry.attempts.pop() {
                entry.attempts.clear();
                entry.attempts.push(last);
            }
        }
    }
    skills::group_entries(&entries)
        .into_values()
        .flatten()
        .collect()
}

/// The export flag writes a grouping of this invocation's selected ledger
/// window. Raw records stay idempotent and carry their own exact skill set.
pub(super) fn export_summary(
    path: &std::path::Path,
    entries: &[LedgerEntry],
) -> anyhow::Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path)?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), &skills::summarize(entries))?;
    temporary.persist(path.join("skill-summary.json"))?;
    Ok(())
}

#[cfg(test)]
mod tests;
