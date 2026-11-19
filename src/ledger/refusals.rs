//! A dispatch refused before any backend ran says "this work item is
//! waiting". That is a state, not an event. A loop that cannot admit a
//! worker selects the same item again on every tick, and recording each
//! refusal as history grew one central's ledger by about two thousand
//! 8 KiB rows a day until every reader of it took minutes. Only the first
//! refusal of an unbroken run is recorded; the row stays the newest one for
//! its work item, so status still shows the item waiting, and since when.

use super::entry::LedgerEntry;
use super::gates::launched_no_backend;
use crate::config::GahConfig;
use anyhow::Result;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// How far back from the ledger's end the previous row for a work item is
/// looked for. A refusal row is under 10 KiB, so this spans a couple of
/// dozen items waiting in turn. A row further back is appended again, which
/// costs one row, not correctness.
const TAIL_BYTES: u64 = 256 * 1024;

/// Appends `entry`, unless it is a refusal that launched nothing and the
/// newest row for the same work item already records that refusal. Returns
/// whether a row was written.
pub fn append_unless_repeated_refusal(cfg: &GahConfig, entry: &LedgerEntry) -> Result<bool> {
    if launched_no_backend(entry) && repeats_newest_row(&cfg.defaults.ledger_path(), entry) {
        return Ok(false);
    }
    super::append(cfg, entry)?;
    Ok(true)
}

/// Whether the newest row in the ledger's tail for `entry`'s work item is
/// the same refusal. A ledger or line that cannot be read answers no.
fn repeats_newest_row(path: &Path, entry: &LedgerEntry) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(len) = file.metadata().map(|metadata| metadata.len()) else {
        return false;
    };
    let start = len.saturating_sub(TAIL_BYTES);
    let mut tail = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut tail).is_err() {
        return false;
    }
    let text = String::from_utf8_lossy(&tail);
    let lines: Vec<&str> = text.lines().collect();
    // A tail that starts inside the file starts inside a row.
    let whole_rows = lines.get(usize::from(start > 0)..).unwrap_or_default();
    whole_rows
        .iter()
        .rev()
        .filter_map(|line| serde_json::from_str::<LedgerEntry>(line).ok())
        .find(|row| {
            row.profile == entry.profile
                && row.repo_id == entry.repo_id
                && row.mode == entry.mode
                && row.work_id == entry.work_id
                && row.branch == entry.branch
        })
        .is_some_and(|row| {
            launched_no_backend(&row) && row.validation_result == entry.validation_result
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(profile: &crate::config::Profile, work_id: &str) -> LedgerEntry {
        let mut entry = LedgerEntry::new("test", profile, "auto", "review", work_id, None, None);
        entry.work_id = Some(work_id.into());
        entry.branch = Some(format!("gah/{work_id}"));
        entry.validation_result = Some("deferred_capacity".into());
        entry.attempts_started = Some(0);
        entry
    }

    #[test]
    fn a_run_of_refusals_is_recorded_once_and_a_real_attempt_ends_the_run() {
        let (_tmp, cfg) = crate::ledger::test_util::test_config();
        let profile = crate::config::tests::test_profile_for_notifications();
        let waiting = refusal(&profile, "#1");
        let rows = || crate::ledger::read_entries(&cfg).unwrap().len();

        assert!(append_unless_repeated_refusal(&cfg, &waiting).unwrap());
        for _ in 0..5 {
            assert!(!append_unless_repeated_refusal(&cfg, &waiting).unwrap());
        }
        assert_eq!(rows(), 1);

        // Another item waiting in between does not restart the first one's run.
        assert!(append_unless_repeated_refusal(&cfg, &refusal(&profile, "#2")).unwrap());
        assert!(!append_unless_repeated_refusal(&cfg, &waiting).unwrap());
        assert_eq!(rows(), 2);

        let mut attempt = refusal(&profile, "#1");
        attempt.validation_result = Some("passed".into());
        attempt.attempts_started = Some(1);
        assert!(append_unless_repeated_refusal(&cfg, &attempt).unwrap());
        assert!(
            append_unless_repeated_refusal(&cfg, &attempt).unwrap(),
            "attempts are history"
        );
        assert!(
            append_unless_repeated_refusal(&cfg, &waiting).unwrap(),
            "a new run of waiting"
        );
        assert!(!append_unless_repeated_refusal(&cfg, &waiting).unwrap());
        assert_eq!(rows(), 5);
    }
}
