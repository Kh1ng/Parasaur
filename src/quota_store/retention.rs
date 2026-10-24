//! #1539: bounded retention for the append-only account quota store.
//!
//! `quota_observations.jsonl` grows by a few records on every refresh tick
//! and nothing ever removed old lines: on the central node the file reached
//! 6.3 MB. Retention compacts it to the latest
//! [`RetentionPolicy::max_per_window`] observations per source identity and
//! window, with a total size bound as the backstop.
//!
//! Selector preservation is a property of *what* is kept, not just how much:
//!
//! - the newest observation of every series is always kept, so
//!   `latest_windows_for_backend`, `latest_windows_for_identity*` and
//!   `latest_for_identity` return the same readings before and after;
//! - credential lifecycle markers (`credential_updated` /
//!   `credential_removed`) are never dropped, so `current_source_records`
//!   keeps resolving a rotated source's current binding;
//! - the last line per credential id is never dropped, because
//!   `current_source_records` reads it as the binding record;
//! - the file order of kept lines is unchanged, because
//!   `current_source_records` derives lifecycle cutoffs from append order.
//!
//! Compaction rewrites the file atomically (temp file + rename) under the
//! same `<state>.lock` cross-process convention `availability.rs` uses;
//! `append_all` takes that lock too, so a rotation can never interleave with
//! a concurrent append. Rotation also holds an exclusive lock on the data
//! file itself, which is the only lock pre-#1539 binaries take, so a
//! mixed-version append cannot be lost to a rename either.

use super::QuotaObservationRecord;
use anyhow::{Context, Result};
use fs2::FileExt;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Latest observations kept per source identity and window. At the ~14 minute
/// refresh cadence this holds several days of local history per window;
/// anything older is already durably exported by the telemetry pipeline.
pub const DEFAULT_MAX_PER_WINDOW: usize = 500;

/// Size bound for the store file. Appends past this bound trigger
/// compaction, and compaction trims unprotected history back under it.
pub const DEFAULT_MAX_BYTES: usize = 4 * 1024 * 1024;

/// Bounded retention for the quota store. Operator knobs (defaults above):
/// `GAH_QUOTA_STORE_MAX_PER_WINDOW`, `GAH_QUOTA_STORE_MAX_BYTES`. An absent
/// or unparsable value falls back to the default; the bound is never
/// silently disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Latest observations kept per source identity and window.
    pub max_per_window: usize,
    /// Upper bound for the store file's size in bytes.
    pub max_bytes: usize,
}

impl RetentionPolicy {
    pub fn from_env() -> Self {
        Self {
            max_per_window: env_bound("GAH_QUOTA_STORE_MAX_PER_WINDOW", DEFAULT_MAX_PER_WINDOW),
            max_bytes: env_bound("GAH_QUOTA_STORE_MAX_BYTES", DEFAULT_MAX_BYTES),
        }
    }
}

fn env_bound(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

/// Lock file next to the store, mirroring `availability.rs`'s convention.
pub(super) fn lock_path_for(state_path: &Path) -> PathBuf {
    let mut lock_name = state_path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "quota_observations.jsonl".into());
    lock_name.push(".lock");
    state_path.with_file_name(lock_name)
}

/// Take the cross-process rotation lock `append_all` also takes. Returned
/// guard unlocks on drop.
pub(super) fn lock_state_file(state_path: &Path) -> Result<File> {
    let lock_path = lock_path_for(state_path);
    let lock_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("opening lock file {}", lock_path.display()))?;
    lock_file
        .lock_exclusive()
        .with_context(|| format!("locking {}", lock_path.display()))?;
    Ok(lock_file)
}

/// Cheap gate for hot paths: compaction is due once the store exceeds its
/// configured size bound. One metadata call, no parse.
pub fn compaction_due(state_path: &Path, policy: RetentionPolicy) -> bool {
    fs::metadata(state_path)
        .map(|meta| meta.len() as usize > policy.max_bytes)
        .unwrap_or(false)
}

/// One parsed JSONL line of the store. `record` is `None` for a malformed
/// line; readers already skip those, and compaction drops them.
struct StoreLine {
    text: String,
    record: Option<QuotaObservationRecord>,
}

/// (backend, credential, instance, pool, model, window): one quota series.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SeriesKey {
    backend: String,
    credential_id: Option<String>,
    backend_instance: Option<String>,
    quota_pool: Option<String>,
    model: Option<String>,
    quota_window: Option<String>,
}

impl SeriesKey {
    fn of(record: &QuotaObservationRecord) -> Self {
        Self {
            backend: record.backend.clone(),
            credential_id: record.credential_id.clone(),
            backend_instance: record.backend_instance.clone(),
            quota_pool: record.quota_pool.clone(),
            model: record.model.clone(),
            quota_window: record.quota_window.clone(),
        }
    }
}

/// Observation time used for "latest" ordering, matching the selectors:
/// `checked_at`, falling back to `observed_at`.
fn observation_time(record: &QuotaObservationRecord) -> Option<OffsetDateTime> {
    record
        .checked_at
        .as_deref()
        .or(record.observed_at.as_deref())
        .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok())
}

/// Latest-first ordering: observation time, then file order. Lines with no
/// parseable time sort as the oldest.
fn latest_order(lines: &[StoreLine], index: usize) -> (bool, Option<OffsetDateTime>, usize) {
    let time = lines[index].record.as_ref().and_then(observation_time);
    (time.is_some(), time, index)
}

fn is_lifecycle_marker(record: &QuotaObservationRecord) -> bool {
    matches!(
        record.usage_source.as_deref(),
        Some("credential_updated" | "credential_removed")
    )
}

/// Pure: which lines survive `policy`. Kept lines keep their file order.
fn plan_retention(lines: &[StoreLine], policy: RetentionPolicy) -> Vec<bool> {
    let mut keep = vec![false; lines.len()];
    // Lines a selector's answer depends on; never dropped by the size
    // backstop either.
    let mut protected = vec![false; lines.len()];
    let mut per_series: BTreeMap<SeriesKey, Vec<usize>> = BTreeMap::new();
    let mut last_per_credential: BTreeMap<Option<String>, usize> = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(record) = &line.record else { continue };
        if is_lifecycle_marker(record) {
            // Dropping a marker would resurrect a rotated-out account's
            // readings (`current_source_records` cuts at the last marker).
            keep[index] = true;
            protected[index] = true;
        }
        if record.credential_id.is_some() {
            // `current_source_records` reads the LAST line of a credential
            // as its current binding.
            last_per_credential.insert(record.credential_id.clone(), index);
        }
        per_series
            .entry(SeriesKey::of(record))
            .or_default()
            .push(index);
    }
    for &index in last_per_credential.values() {
        keep[index] = true;
        protected[index] = true;
    }
    let max_per_window = policy.max_per_window.max(1);
    for indices in per_series.values() {
        let mut ordered = indices.clone();
        ordered.sort_by_key(|&index| latest_order(lines, index));
        // The newest N observations of the series (the newest is always in
        // the kept set, so every selector keeps its answer).
        for &index in ordered.iter().rev().take(max_per_window) {
            keep[index] = true;
        }
        // The newest reading must also survive the size backstop.
        if let Some(&newest) = ordered.last() {
            protected[newest] = true;
        }
    }
    // Size backstop: trim the oldest unprotected history until the file fits.
    let line_bytes = |index: usize| lines[index].text.len() + 1;
    let mut kept_bytes: usize = (0..lines.len())
        .filter(|&index| keep[index])
        .map(line_bytes)
        .sum();
    if kept_bytes > policy.max_bytes {
        let mut droppable: Vec<usize> = (0..lines.len())
            .filter(|&index| keep[index] && !protected[index])
            .collect();
        droppable.sort_by_key(|&index| latest_order(lines, index));
        for index in droppable {
            if kept_bytes <= policy.max_bytes {
                break;
            }
            keep[index] = false;
            kept_bytes -= line_bytes(index);
        }
    }
    keep
}

fn read_store_lines(state_path: &Path) -> Result<Vec<StoreLine>> {
    let content = match fs::read_to_string(state_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error).context("read quota store"),
    };
    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|text| StoreLine {
            text: text.to_string(),
            record: serde_json::from_str(text).ok(),
        })
        .collect())
}

/// Would compaction drop anything under `policy`?
fn drops_records(state_path: &Path, policy: RetentionPolicy) -> Result<bool> {
    let lines = read_store_lines(state_path)?;
    Ok(plan_retention(&lines, policy).iter().any(|keep| !keep))
}

/// Compact the store to `policy`, returning how many lines were dropped.
///
/// When compaction would drop records and `export_first` is provided, the
/// telemetry export runs BEFORE the rewrite (#1539: export runs before
/// compaction), so no observation is compacted away before it was exported;
/// a failed export aborts compaction and leaves the store untouched. The
/// export is checked without the store lock so appends never stall behind
/// it: appends only add records, the export still sees every
/// pre-compaction record, and records appended after the export read are
/// the newest of their series, which retention always keeps.
pub fn compact(
    state_path: &Path,
    policy: RetentionPolicy,
    export_first: Option<&crate::config::GahConfig>,
) -> Result<usize> {
    if !state_path.exists() {
        return Ok(0);
    }
    if let Some(cfg) = export_first {
        if drops_records(state_path, policy)? {
            crate::telemetry::health::export_for_retention(cfg)?;
        }
    }
    let lock_file = lock_state_file(state_path)?;
    let result = compact_locked(state_path, policy);
    FileExt::unlock(&lock_file).ok();
    result
}

fn compact_locked(state_path: &Path, policy: RetentionPolicy) -> Result<usize> {
    let lines = read_store_lines(state_path)?;
    let keep = plan_retention(&lines, policy);
    let dropped = lines.iter().zip(&keep).filter(|(_, keep)| !**keep).count();
    if dropped == 0 {
        return Ok(0);
    }
    let mut out = String::new();
    for (line, keep) in lines.iter().zip(&keep) {
        if *keep {
            out.push_str(&line.text);
            out.push('\n');
        }
    }
    // Pre-#1539 binaries lock only the data file; hold that lock across the
    // replace so a mixed-version append cannot be lost to the rename.
    let data = OpenOptions::new()
        .read(true)
        .open(state_path)
        .with_context(|| format!("opening {}", state_path.display()))?;
    data.lock_exclusive()
        .with_context(|| format!("locking {}", state_path.display()))?;
    let replaced = replace_state_file(state_path, &out);
    let _ = FileExt::unlock(&data);
    replaced?;
    Ok(dropped)
}

/// Atomic replace: temp file, fsync, rename (the `availability.rs` convention).
fn replace_state_file(state_path: &Path, content: &str) -> Result<()> {
    let dir = state_path.parent().unwrap_or_else(|| Path::new("."));
    let tmp_path = dir.join(format!(
        "{}.tmp.{}",
        state_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("quota_observations.jsonl"),
        std::process::id()
    ));
    {
        let mut tmp = File::create(&tmp_path)
            .with_context(|| format!("creating temp file {}", tmp_path.display()))?;
        tmp.write_all(content.as_bytes())
            .with_context(|| format!("writing temp file {}", tmp_path.display()))?;
        tmp.sync_all().ok();
    }
    fs::rename(&tmp_path, state_path).with_context(|| {
        format!(
            "renaming {} to {}",
            tmp_path.display(),
            state_path.display()
        )
    })
}

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;
