//! Prune-only quota retention. Appends and compaction acquire the sidecar lock
//! before opening the store. Pre-sidecar-lock binaries must be stopped before
//! maintenance: a data-file flock cannot protect an old handle across rename.
use super::{has_quota_data, QuotaObservationRecord};
use anyhow::{Context, Result};
use fs2::FileExt;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

/// Recent history plus selector dependencies, with no count or byte cap.
#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicy {
    pub max_age: Duration,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_age: Duration::days(14),
        }
    }
}

pub(super) fn lock_path_for(state_path: &Path) -> PathBuf {
    let mut name = state_path.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    state_path.with_file_name(name)
}

/// Acquire before opening the data file; dropping the guard releases the lock.
pub(super) fn lock_state_file(state_path: &Path) -> Result<File> {
    let lock_path = lock_path_for(state_path);
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("opening lock file {}", lock_path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("locking {}", lock_path.display()))?;
    Ok(file)
}

struct StoreLine {
    text: String,
    record: Option<QuotaObservationRecord>,
}

type SeriesKey<'a> = (
    &'a str,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
);

fn series_key(record: &QuotaObservationRecord) -> SeriesKey<'_> {
    (
        &record.backend,
        record.credential_id.as_deref(),
        record.backend_instance.as_deref(),
        record.quota_pool.as_deref(),
        record.model.as_deref(),
        record.quota_window.as_deref(),
    )
}

fn observation_time(record: &QuotaObservationRecord) -> Option<OffsetDateTime> {
    record
        .checked_at
        .as_deref()
        .or(record.observed_at.as_deref())
        .and_then(|value| OffsetDateTime::parse(value, &Rfc3339).ok())
}

/// Preserve selector dependencies independently of recent history and file order.
fn plan_retention(records: &[QuotaObservationRecord], cutoff: OffsetDateTime) -> BTreeSet<usize> {
    let mut keep = BTreeSet::new();
    let mut bindings = BTreeMap::new();
    let mut lifecycle = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        if observation_time(record).is_none_or(|time| time >= cutoff) {
            keep.insert(index);
        }
        if let Some(id) = record.credential_id.as_deref() {
            // current_source_records uses the last append as the binding and
            // only the last lifecycle marker as its cutoff, even after clock rollback.
            bindings.insert(id, index);
            if matches!(
                record.usage_source.as_deref(),
                Some("credential_updated" | "credential_removed")
            ) {
                lifecycle.insert(id, index);
            }
        }
    }
    keep.extend(bindings.into_values());
    keep.extend(lifecycle.into_values());

    // Apply lifecycle/binding filtering BEFORE freshness ordering. A future-dated
    // pre-rotation reading must never displace the current window's winner.
    let current: std::collections::HashSet<_> = super::current_source_records(records)
        .into_iter()
        .map(|record| record as *const _)
        .collect();
    let mut newest = BTreeMap::new();
    let mut successful = BTreeMap::new();
    let mut invalidations = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        if !current.contains(&(record as *const _)) {
            continue;
        }
        let order = (observation_time(record), index);
        let key = series_key(record);
        newest
            .entry(key)
            .and_modify(|old| *old = order.max(*old))
            .or_insert(order);
        if record.check_error.is_none() && has_quota_data(record) {
            // A timestamp-less failure cannot invalidate a successful reading.
            successful
                .entry(key)
                .and_modify(|old| *old = order.max(*old))
                .or_insert(order);
        } else if order.0.is_some() {
            // select_latest_windows invalidates at >= time, including equal-time
            // failures and window-less failures followed by window-less successes.
            // Keep the maximum per full source/model/window: identity selection
            // can merge these scopes, backend selection keeps them separate.
            invalidations
                .entry(key)
                .and_modify(|old| *old = order.max(*old))
                .or_insert(order);
        }
    }
    keep.extend(
        newest
            .into_values()
            .chain(successful.into_values())
            .chain(invalidations.into_values())
            .map(|(_, index)| index),
    );
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

/// Read once under the append lock, export that exact snapshot, then atomically
/// replace it before unlocking. Export/sync failures leave the store untouched.
/// Only prune calls this; appends never delete unexported history.
pub fn compact(
    state_path: &Path,
    policy: RetentionPolicy,
    cfg: &crate::config::GahConfig,
) -> Result<usize> {
    compact_with_export(state_path, policy, |records| {
        crate::telemetry::health::export_for_retention(cfg, records)
    })
}

fn compact_with_export(
    state_path: &Path,
    policy: RetentionPolicy,
    export: impl FnOnce(&[QuotaObservationRecord]) -> Result<()>,
) -> Result<usize> {
    if !state_path.try_exists()? {
        return Ok(0);
    }
    let _lock = lock_state_file(state_path)?;
    let lines = read_store_lines(state_path)?;
    let records: Vec<_> = lines
        .iter()
        .filter_map(|line| line.record.clone())
        .collect();
    let cutoff = OffsetDateTime::now_utc()
        .checked_sub(policy.max_age)
        .unwrap_or(time::Date::MIN.midnight().assume_utc());
    let keep = plan_retention(&records, cutoff);
    let dropped = lines.len() - keep.len();
    if dropped == 0 {
        return Ok(0);
    }
    export(&records)?;
    let mut index = 0;
    let mut out = String::new();
    for line in &lines {
        if line.record.is_some() {
            if keep.contains(&index) {
                out.push_str(&line.text);
                out.push('\n');
            }
            index += 1;
        }
    }
    replace_state_file(state_path, &out)?;
    Ok(dropped)
}

fn replace_state_file(state_path: &Path, content: &str) -> Result<()> {
    let tmp_path = state_path.with_extension(format!("tmp.{}", std::process::id()));
    let mut tmp = File::create(&tmp_path).context("creating quota temp file")?;
    tmp.write_all(content.as_bytes())
        .context("writing quota temp file")?;
    tmp.sync_all().context("syncing quota temp file")?;
    fs::rename(&tmp_path, state_path).context("replacing quota store")
}

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;
