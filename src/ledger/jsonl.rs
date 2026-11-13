use crate::config::GahConfig;
use crate::job_kind::JobKind;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{LazyLock, Mutex};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::entry::LedgerEntry;
use super::locking;

#[cfg(test)]
static READ_ENTRIES_CALLS: LazyLock<Mutex<std::collections::HashMap<PathBuf, usize>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

/// Resolve current paid-route grants for one work item. Later grant/revoke
/// entries supersede earlier ones for the same exact backend/model identity.
pub fn active_paid_route_approvals(
    cfg: &GahConfig,
    profile_name: &str,
    work_id: &str,
) -> Result<HashSet<(String, Option<String>)>> {
    let entries = read_entries(cfg)?;
    Ok(active_paid_route_approvals_from_entries(
        &entries,
        profile_name,
        work_id,
    ))
}

pub fn active_paid_route_approvals_from_entries(
    entries: &[LedgerEntry],
    profile_name: &str,
    work_id: &str,
) -> HashSet<(String, Option<String>)> {
    let mut active = HashSet::new();
    let aliases = work_id_aliases(work_id);
    for entry in entries {
        if entry.profile != profile_name
            || !entry
                .work_id
                .as_deref()
                .is_some_and(|id| aliases.iter().any(|alias| alias == id))
        {
            continue;
        }
        let identity = (
            entry.effective_backend.clone(),
            entry.effective_model.clone(),
        );
        match entry.mode.as_str() {
            "paid_route_approval_grant" => {
                active.insert(identity);
            }
            "paid_route_approval_revoke" => {
                active.remove(&identity);
            }
            _ => {}
        }
    }
    active
}

/// A manager review is a much shorter-lived action than a full dispatch
/// attempt (`CLAIM_STALE_AFTER_HOURS` in dispatch.rs is 6h) -- 2h is a
/// generous margin for a real review session while still releasing a hold
/// left behind by a crashed/forgotten manager session same-day.
pub const REVIEW_HOLD_STALE_AFTER_HOURS: i64 = 2;

fn is_review_hold_stale(entry: &LedgerEntry) -> bool {
    let entry_time = if let Ok(parsed) = OffsetDateTime::parse(&entry.timestamp, &Rfc3339) {
        parsed
    } else if let Ok(secs) = entry.timestamp.parse::<i64>() {
        if let Ok(dt) = OffsetDateTime::from_unix_timestamp(secs) {
            dt
        } else {
            return true;
        }
    } else {
        return true;
    };
    let now = crate::clock::now_utc();
    now - entry_time > time::Duration::hours(REVIEW_HOLD_STALE_AFTER_HOURS)
}

/// Scans this profile's ledger entries for active review holds (`gah hold
/// set` with no later `gah hold clear`, and not yet stale) so
/// `decide_next_action` can skip auto-merging a work_id a manager session is
/// actively reviewing out of band. Entries are appended in chronological
/// order, so a single forward pass where each hold/release overwrites the
/// previous verdict for its work_id naturally lands on the latest one.
pub fn active_review_hold_work_ids(
    cfg: &GahConfig,
    profile_name: &str,
) -> std::collections::HashSet<String> {
    let entries = match read_entries(cfg) {
        Ok(entries) => entries,
        Err(_) => return std::collections::HashSet::new(),
    };
    active_review_hold_work_ids_from_entries(&entries, profile_name)
}

pub fn active_review_hold_work_ids_from_entries(
    entries: &[LedgerEntry],
    profile_name: &str,
) -> std::collections::HashSet<String> {
    let mut held: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    for entry in entries.iter().filter(|e| e.profile == profile_name) {
        let Some(work_id) = entry.work_id.as_deref() else {
            continue;
        };
        match entry.mode.as_str() {
            "review_hold" => {
                held.insert(work_id.to_string(), !is_review_hold_stale(entry));
            }
            "review_hold_release" => {
                held.insert(work_id.to_string(), false);
            }
            _ => {}
        }
    }

    held.into_iter()
        .filter_map(|(work_id, active)| active.then_some(work_id))
        .collect()
}

fn append_locked(path: &Path, entry: &LedgerEntry) -> Result<()> {
    let tail = final_record(path)?;
    if let Some(offset) = truncated_tail_offset(&tail.bytes) {
        anyhow::bail!(
            "ledger {} has an unterminated invalid final record at byte {}; run `gah ledger repair-tail` before appending",
            path.display(),
            tail.start + offset as u64
        );
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening ledger {}", path.display()))?;
    let normalized = entry.normalized_for_persistence();
    let mut value = serde_json::to_value(&normalized).context("serializing ledger entry")?;
    crate::redact::redact_json_value(&mut value);
    // One write per record: a record split across writes can be torn by a
    // kill or ENOSPC between them. A complete final record whose newline was
    // lost is framed first, so the new record never joins its line.
    let mut record = Vec::new();
    if !tail.bytes.is_empty() && !tail.bytes.ends_with(b"\n") {
        record.push(b'\n');
    }
    serde_json::to_writer(&mut record, &value).context("serializing ledger entry")?;
    record.push(b'\n');
    file.write_all(&record).context("writing ledger record")?;
    Ok(())
}

/// The ledger's final record (everything after its last newline, or the
/// final newline-terminated line's last byte), read from the end so an
/// append does not re-read the whole ledger.
struct FinalRecord {
    start: u64,
    bytes: Vec<u8>,
}

fn final_record(path: &Path) -> Result<FinalRecord> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(FinalRecord {
                start: 0,
                bytes: Vec::new(),
            })
        }
        Err(err) => return Err(err).with_context(|| format!("reading ledger {}", path.display())),
    };
    let len = file.metadata()?.len();
    let mut start = len;
    // Chunks are collected newest-first and joined once, so a long final
    // record costs one copy, not one per chunk.
    let mut chunks: Vec<Vec<u8>> = Vec::new();
    const CHUNK: u64 = 64 * 1024;
    while start > 0 {
        let read_from = start.saturating_sub(CHUNK);
        let mut chunk = vec![0; (start - read_from) as usize];
        file.seek(SeekFrom::Start(read_from))?;
        file.read_exact(&mut chunk)?;
        start = read_from;
        // The byte that ends the previous record is enough context: a final
        // newline means the tail is framed, otherwise the record runs back
        // to the previous newline.
        let is_tail = chunks.is_empty();
        let search_end = if is_tail {
            chunk.len().saturating_sub(1)
        } else {
            chunk.len()
        };
        let boundary = (is_tail && chunk.ends_with(b"\n")) || chunk[..search_end].contains(&b'\n');
        chunks.push(chunk);
        if boundary {
            break;
        }
    }
    let mut bytes: Vec<u8> = Vec::with_capacity(chunks.iter().map(Vec::len).sum());
    for chunk in chunks.iter().rev() {
        bytes.extend_from_slice(chunk);
    }
    if bytes.ends_with(b"\n") {
        return Ok(FinalRecord {
            start: len - 1,
            bytes: b"\n".to_vec(),
        });
    }
    if let Some(idx) = bytes.iter().rposition(|byte| *byte == b'\n') {
        bytes.drain(..=idx);
        start += idx as u64 + 1;
    }
    Ok(FinalRecord { start, bytes })
}

fn sync_mirror(cfg: &GahConfig) {
    if let Err(err) = super::sqlite_store::sync_from_jsonl(cfg) {
        eprintln!("warning: failed to sync sqlite ledger mirror: {err:#}");
    }
}

pub fn append(cfg: &GahConfig, entry: &LedgerEntry) -> Result<PathBuf> {
    let path = cfg.defaults.ledger_path();
    {
        let _lock = locking::exclusive(&path)?;
        append_locked(&path, entry)?;
    }
    sync_mirror(cfg);
    super::dispatch_notify::notify_if_genuine_failure(cfg, entry); // see its module doc
    Ok(path)
}

/// Import one terminal native result under the canonical ledger lock. A replay
/// compares the immutable source receipt digest, not a second sidecar whose
/// acknowledgement could be lost after the ledger append. History is streamed
/// and fully validated before any write, including unrelated discarded rows.
pub(super) fn append_received_dispatch(
    cfg: &GahConfig,
    entry: &LedgerEntry,
) -> Result<super::remote_dispatch::ReceiveOutcome> {
    use super::remote_dispatch::ReceiveOutcome;
    let path = cfg.defaults.ledger_path();
    let lock = locking::exclusive(&path)?;
    let mut found = false;
    match fs::File::open(&path) {
        Ok(file) => {
            visit_jsonl_reader(BufReader::new(file), &path, 0, |existing| {
                // Local claim/administrative records may share the native run
                // id. Another terminal dispatch may not impersonate this run.
                if existing.session_id == entry.session_id
                    && (crate::job_kind::JobKind::parse(&existing.mode).is_ok()
                        || existing.mode == "handoff"
                        || existing.remote_execution.is_some())
                {
                    // Central review backfill can legitimately update business
                    // fields after import. The original received digest remains
                    // immutable, so that update cannot break lost-ack replay.
                    anyhow::ensure!(
                        existing.remote_execution == entry.remote_execution,
                        "A different terminal result already owns this native run"
                    );
                    anyhow::ensure!(
                        !found,
                        "The canonical ledger contains duplicate native results"
                    );
                    found = true;
                }
                Ok(())
            })?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("reading canonical dispatch history"),
    }
    if !found {
        append_locked(&path, entry)?;
    }
    // A retry also flushes an existing record: the first delivery may have
    // failed after writing it but before confirming durability to its caller.
    let durable_path = fs::canonicalize(&path)?;
    OpenOptions::new()
        .write(true)
        .open(&durable_path)?
        .sync_all()?;
    let parent = durable_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::File::open(parent)?.sync_all()?;
    drop(lock);
    sync_mirror(cfg);
    if !found {
        super::dispatch_notify::notify_if_genuine_failure(cfg, entry);
    }
    Ok(if found {
        ReceiveOutcome::AlreadyRecorded
    } else {
        ReceiveOutcome::Recorded
    })
}

/// Record a validated external approval without racing another transition.
/// Returns the persisted grant, including bounds inherited from its request.
pub use super::gates::{
    effective_human_gate_from_entries, effective_human_gate_from_index, index_entries_by_work_id,
    is_entry_stale, work_id_aliases, EffectiveHumanGate, LedgerEntriesByWorkId,
};

pub fn append_external_approval(
    cfg: &GahConfig,
    mut entry: LedgerEntry,
) -> Result<(LedgerEntry, PathBuf)> {
    let path = cfg.defaults.ledger_path();
    {
        let _lock = locking::exclusive(&path)?;
        let entries = read_entries_unlocked(&path)?;
        super::approvals::prepare_external_approval(&entries, &mut entry)?;
        append_locked(&path, &entry)?;
    }
    sync_mirror(cfg);
    Ok((entry, path))
}

/// Atomically append a human gate only when the same work item has no active
/// effective gate. The check and append share the ledger's cross-process lock,
/// so concurrent controller slots cannot both observe "ungated" and write
/// duplicate transitions.
pub fn append_human_gate_if_transition(cfg: &GahConfig, entry: &LedgerEntry) -> Result<bool> {
    anyhow::ensure!(
        entry.human_required,
        "conditional gate entry must require human action"
    );
    let work_id = entry
        .work_id
        .as_deref()
        .context("conditional gate entry must have a work_id")?;
    let path = cfg.defaults.ledger_path();
    let _lock = locking::exclusive(&path)?;
    let existing = read_entries_unlocked(&path)?;
    if let Some(active) =
        effective_human_gate_from_entries(&existing, &entry.profile, &entry.repo_id, work_id)
    {
        let active_is_review_derived = JobKind::parse(&active.mode) == Ok(JobKind::Review)
            || active.reason_code.as_deref() == Some("stuck_loop_gate");
        let starts_new_review_generation = entry.review_generation.is_some()
            && active_is_review_derived
            && (active.review_contract_version != entry.review_contract_version
                || active.review_generation != entry.review_generation);
        if !starts_new_review_generation {
            return Ok(false);
        }
    }
    append_locked(&path, entry)?;
    drop(_lock);
    sync_mirror(cfg);
    Ok(true)
}

/// Result of the deliberately narrow JSONL tail-repair operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailRepair {
    pub backup_path: Option<PathBuf>,
    pub dropped_bytes: usize,
}

/// Back up and remove a single unterminated, invalid final JSONL record.
///
/// A full disk or abrupt process death can leave the last `write_all` only
/// partially persisted. We never repair an invalid record followed by a
/// newline (that is structural corruption, not a torn append), and we never
/// touch any valid final record. The rejected bytes are retained beside the
/// ledger before truncation for audit/recovery.
pub fn repair_truncated_tail(cfg: &GahConfig, dry_run: bool) -> Result<TailRepair> {
    let path = cfg.defaults.ledger_path();
    if !path.exists() {
        return Ok(TailRepair {
            backup_path: None,
            dropped_bytes: 0,
        });
    }
    let _lock = locking::exclusive(&path)?;
    repair_truncated_tail_at(&path, dry_run)
}

fn repair_truncated_tail_at(path: &Path, dry_run: bool) -> Result<TailRepair> {
    let bytes = fs::read(path).with_context(|| format!("reading ledger {}", path.display()))?;
    let Some(offset) = truncated_tail_offset(&bytes) else {
        return Ok(TailRepair {
            backup_path: None,
            dropped_bytes: 0,
        });
    };
    let tail = &bytes[offset..];
    let backup_path = path.with_file_name(format!(
        "{}.corrupt-tail-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("ledger"),
        OffsetDateTime::now_utc().unix_timestamp_nanos(),
    ));
    if !dry_run {
        fs::write(&backup_path, tail).with_context(|| {
            format!(
                "backing up truncated ledger tail to {}",
                backup_path.display()
            )
        })?;
        let file = OpenOptions::new()
            .write(true)
            .open(path)
            .with_context(|| format!("opening ledger {} for tail repair", path.display()))?;
        file.set_len(offset as u64)
            .with_context(|| format!("truncating ledger {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("syncing repaired ledger {}", path.display()))?;
    }
    Ok(TailRepair {
        backup_path: Some(backup_path),
        dropped_bytes: tail.len(),
    })
}

/// Returns the byte offset of a physically torn final JSONL record. A final
/// newline means the record was fully framed, so any invalid content is not
/// safe to discard automatically.
fn truncated_tail_offset(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        return None;
    }
    let offset = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |idx| idx + 1);
    let tail = &bytes[offset..];
    if serde_json::from_slice::<serde_json::Value>(tail).is_err() {
        Some(offset)
    } else {
        None
    }
}

pub(super) fn parse_jsonl_entries(
    text: &str,
    path: &Path,
    starting_line: usize,
) -> Result<Vec<LedgerEntry>> {
    Ok(parse_jsonl_reader(text.as_bytes(), path, starting_line)?.entries)
}

/// Preserve full records and physical byte offsets without retaining a second
/// whole-journal text allocation. One reusable line buffer handles framing.
fn parse_jsonl_reader(
    reader: impl BufRead,
    path: &Path,
    starting_line: usize,
) -> Result<LedgerSnapshot> {
    let mut entries = vec![];
    let byte_len = visit_jsonl_reader(reader, path, starting_line, |entry| {
        entries.push(entry);
        Ok(())
    })?;
    Ok(LedgerSnapshot { entries, byte_len })
}

fn visit_jsonl_reader(
    mut reader: impl BufRead,
    path: &Path,
    starting_line: usize,
    mut visit: impl FnMut(LedgerEntry) -> Result<()>,
) -> Result<u64> {
    let mut line = String::new();
    let mut byte_len = 0;
    let mut line_number = starting_line;
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .with_context(|| format!("reading {}", path.display()))?;
        if bytes == 0 {
            break;
        }
        byte_len += bytes as u64;
        line_number += 1;
        let line = line.strip_suffix('\n').unwrap_or(&line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim().is_empty() {
            continue;
        }
        let entry = serde_json::from_str::<LedgerEntry>(line).with_context(|| {
            format!(
                "parsing ledger entry {} from {}",
                line_number,
                path.display()
            )
        })?;
        visit(entry)?;
    }
    Ok(byte_len)
}

pub(super) struct LedgerSnapshot {
    pub(super) entries: Vec<LedgerEntry>,
    pub(super) byte_len: u64,
}

pub(super) fn read_snapshot(cfg: &GahConfig) -> Result<LedgerSnapshot> {
    let path = cfg.defaults.ledger_path();
    #[cfg(test)]
    {
        *READ_ENTRIES_CALLS
            .lock()
            .expect("ledger read counter lock poisoned")
            .entry(path.clone())
            .or_default() += 1;
    }
    let _lock = locking::shared(&path)?;
    read_snapshot_unlocked(&path)
}

fn read_snapshot_unlocked(path: &Path) -> Result<LedgerSnapshot> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LedgerSnapshot {
                entries: vec![],
                byte_len: 0,
            })
        }
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    parse_jsonl_reader(BufReader::new(file), path, 0)
}

/// Visit every validated record under the ledger read lock without retaining history.
/// Callbacks must not write the ledger; stage external effects until this succeeds.
pub(crate) fn visit_entries(
    cfg: &GahConfig,
    visit: impl FnMut(LedgerEntry) -> Result<()>,
) -> Result<()> {
    let path = cfg.defaults.ledger_path();
    #[cfg(test)]
    {
        *READ_ENTRIES_CALLS
            .lock()
            .expect("ledger read counter lock poisoned")
            .entry(path.clone())
            .or_default() += 1;
    }
    let _lock = locking::shared(&path)?;
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    visit_jsonl_reader(BufReader::new(file), &path, 0, visit)?;
    Ok(())
}

/// Retain matching records in append order, still validating discarded history.
pub(crate) fn read_matching_entries(
    cfg: &GahConfig,
    mut retain: impl FnMut(&LedgerEntry) -> bool,
) -> Result<Vec<LedgerEntry>> {
    let mut entries = Vec::new();
    visit_entries(cfg, |entry| {
        if retain(&entry) {
            entries.push(entry);
        }
        Ok(())
    })?;
    Ok(entries)
}

pub fn read_entries(cfg: &GahConfig) -> Result<Vec<LedgerEntry>> {
    Ok(read_snapshot(cfg)?.entries)
}

pub(super) fn read_entries_unlocked(path: &Path) -> Result<Vec<LedgerEntry>> {
    Ok(read_snapshot_unlocked(path)?.entries)
}

#[cfg(test)]
pub(crate) fn reset_read_entries_call_count(cfg: &GahConfig) {
    READ_ENTRIES_CALLS
        .lock()
        .expect("ledger read counter lock poisoned")
        .remove(&cfg.defaults.ledger_path());
}

#[cfg(test)]
pub(crate) fn read_entries_call_count(cfg: &GahConfig) -> usize {
    READ_ENTRIES_CALLS
        .lock()
        .expect("ledger read counter lock poisoned")
        .get(&cfg.defaults.ledger_path())
        .copied()
        .unwrap_or(0)
}

/// TICKET-125: review mode's own ledger entry records the reviewer's
/// backend/model, not the implementation's -- grouping/cost-vs-quality
/// reporting needs the verdict attributed back to whichever backend
/// actually wrote the code being reviewed. Finds the most recent fix/improve
/// entry for `branch` that doesn't already have a verdict and updates it
/// in place (the ledger has no other mutation path today; this is the one
/// exception, and it's rare enough -- once per review completion -- not to
/// need more than a full read-modify-write of the file, which
/// `rewrite_locked` replaces atomically).
pub struct ReviewVerdictBackfill<'a> {
    pub verdict: &'a str,
    pub confidence: &'a str,
    pub reviewer_backend: &'a str,
    pub reviewer_model: Option<&'a str>,
    pub reviewer_tier: Option<&'a str>,
    pub review_gate_reason: Option<&'a str>,
    pub review_source_sha: Option<&'a str>,
    pub review_metadata_fingerprint: Option<&'a str>,
    pub review_contract_version: Option<u32>,
    pub review_generation: Option<&'a str>,
    pub blocking_findings: &'a [String],
    pub actionable_findings: &'a [crate::models::ActionableReviewFinding],
    pub non_blocking_findings: &'a [String],
    pub risk_notes: &'a [String],
    pub evidence: &'a [String],
    pub compatibility_evidence: &'a [String],
}

pub fn backfill_review_verdict(
    cfg: &GahConfig,
    branch: &str,
    backfill: ReviewVerdictBackfill<'_>,
) -> Result<bool> {
    let path = cfg.defaults.ledger_path();
    let lock_file = locking::exclusive(&path)?;
    let mut entries = read_entries_unlocked(&path)?;
    let target_idx = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.branch.as_deref() == Some(branch)
                && matches!(
                    JobKind::parse(&e.mode),
                    Ok(JobKind::Fix) | Ok(JobKind::Improve)
                )
        })
        .max_by_key(|(_, e)| e.timestamp.clone())
        .map(|(idx, _)| idx);

    let Some(idx) = target_idx else {
        return Ok(false);
    };
    entries[idx].review_verdict = Some(backfill.verdict.to_string());
    entries[idx].review_confidence = Some(backfill.confidence.to_string());
    entries[idx].reviewer_backend = Some(backfill.reviewer_backend.to_string());
    entries[idx].reviewer_model = backfill.reviewer_model.map(str::to_string);
    entries[idx].reviewer_tier = backfill.reviewer_tier.map(str::to_string);
    entries[idx].review_gate_reason = backfill.review_gate_reason.map(str::to_string);
    entries[idx].review_source_sha = backfill.review_source_sha.map(str::to_string);
    entries[idx].review_metadata_fingerprint =
        backfill.review_metadata_fingerprint.map(str::to_string);
    entries[idx].review_contract_version = backfill.review_contract_version;
    entries[idx].review_generation = backfill.review_generation.map(str::to_string);
    entries[idx].review_blocking_findings = backfill.blocking_findings.to_vec();
    entries[idx].review_actionable_findings = backfill.actionable_findings.to_vec();
    entries[idx].review_non_blocking_findings = backfill.non_blocking_findings.to_vec();
    entries[idx].review_risk_notes = backfill.risk_notes.to_vec();
    entries[idx].review_evidence = backfill.evidence.to_vec();
    entries[idx].review_compatibility_evidence = backfill.compatibility_evidence.to_vec();

    rewrite_locked(&path, |out| {
        for entry in &entries {
            let mut value = serde_json::to_value(entry).context("serializing ledger entry")?;
            crate::redact::redact_json_value(&mut value);
            serde_json::to_writer(&mut *out, &value).context("serializing ledger entry")?;
            out.write_all(b"\n").context("writing ledger record")?;
        }
        Ok(())
    })?;
    drop(lock_file);

    if let Err(err) = super::sqlite_store::rebuild_from_jsonl(cfg) {
        eprintln!("warning: failed to sync sqlite ledger mirror: {err:#}");
    }

    Ok(true)
}

/// Replace the whole ledger while the caller holds its exclusive lock. The
/// new contents are staged beside the ledger, synced, and renamed over it,
/// so a kill or full disk mid-write leaves the previous ledger intact and a
/// reader sees either the old file or the new one. The ledger keeps its
/// file mode; a symlinked ledger keeps its link and its target is replaced.
fn rewrite_locked(path: &Path, write: impl FnOnce(&mut dyn Write) -> Result<()>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let target =
        fs::canonicalize(path).with_context(|| format!("resolving ledger {}", path.display()))?;
    let mode = fs::metadata(&target)
        .with_context(|| format!("reading ledger {}", target.display()))?
        .permissions()
        .mode()
        & 0o7777;
    crate::installer::files::replace_with(&target, mode, |file| {
        let mut out = std::io::BufWriter::new(file);
        write(&mut out)?;
        out.flush().context("writing ledger")
    })
    .with_context(|| format!("rewriting ledger {}", path.display()))
}

/// TICKET-096: the query sync/reconciliation needs to associate a
/// `SyncMr.work_id` (extracted from a PR/MR title) back to the ledger
/// entries that dispatched it. No new sync-side structure required.
pub fn entries_for_work_id(cfg: &GahConfig, work_id: &str) -> Result<Vec<LedgerEntry>> {
    let aliases = work_id_aliases(work_id);
    read_matching_entries(cfg, |e| {
        e.work_id
            .as_deref()
            .is_some_and(|entry_id| aliases.iter().any(|alias| alias == entry_id))
    })
}

/// True only when this profile/repository's exact work item and immutable
/// source commit already received a completed review from the same authority
/// class after its latest operator reset. Missing legacy attribution fails
/// open: it may cost a review, but never suppresses one.
pub fn review_already_exists(
    cfg: &GahConfig,
    profile_name: &str,
    repo_id: &str,
    work_id: &str,
    source_sha: &str,
    metadata_fingerprint: &str,
    reviewer_class: &str,
) -> Result<bool> {
    let aliases = work_id_aliases(work_id);
    let generation = super::review_generation(Some(source_sha), Some(metadata_fingerprint));
    let entries = read_matching_entries(cfg, |entry| {
        entry.profile == profile_name
            && entry.repo_id == repo_id
            && entry
                .work_id
                .as_deref()
                .is_some_and(|id| aliases.iter().any(|alias| alias == id))
    })?;
    let reset_index = entries.iter().rposition(|entry| {
        entry.profile == profile_name
            && entry.repo_id == repo_id
            && entry.mode == "clear_attempts"
            && entry
                .work_id
                .as_deref()
                .is_some_and(|id| aliases.iter().any(|alias| alias == id))
    });
    let active_entries = reset_index.map_or(entries.as_slice(), |index| &entries[index + 1..]);
    Ok(active_entries.iter().any(|entry| {
        entry.profile == profile_name
            && entry.repo_id == repo_id
            && JobKind::parse(&entry.mode) == Ok(JobKind::Review)
            && entry.review_contract_version.unwrap_or(0)
                >= super::entry::CURRENT_REVIEW_CONTRACT_VERSION
            && entry
                .work_id
                .as_deref()
                .is_some_and(|id| aliases.iter().any(|alias| alias == id))
            && entry.review_source_sha.as_deref() == Some(source_sha)
            && entry.review_metadata_fingerprint.as_deref() == Some(metadata_fingerprint)
            && entry.review_contract_version == Some(super::REVIEW_CONTRACT_VERSION)
            && entry.review_generation == generation
            && entry.reviewer_class.as_deref() == Some(reviewer_class)
            && review_is_dedup_eligible(entry)
    }))
}

fn review_is_dedup_eligible(entry: &LedgerEntry) -> bool {
    // The reviewer may have completed while publication of its comment or
    // controller labels failed. That is not a completed review from the
    // controller's perspective: suppressing the next dispatch would leave
    // the provider state requesting review forever.
    if entry.failure_class.is_some() {
        return false;
    }
    match entry.review_verdict.as_deref() {
        Some("NEEDS_FIX" | "REJECT") => !entry.review_blocking_findings.is_empty(),
        Some("REVIEW_OUTPUT_INVALID") => false,
        Some(_) => true,
        None => false,
    }
}

#[cfg(test)]
#[path = "jsonl/concurrency_tests.rs"]
mod concurrency_tests;

#[cfg(test)]
#[path = "jsonl/rewrite_tests.rs"]
mod rewrite_tests;

#[cfg(test)]
mod tests;
