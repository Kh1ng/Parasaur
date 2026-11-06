//! Canonical execution usage for the combined chat/factory report. Ledger
//! records are visited under their read lock; only grouped readings survive.
use crate::config::GahConfig;
use crate::ledger::{self, LedgerEntry, LedgerUsage, UsageUnknownReason};
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct Identity {
    profile: String,
    project_provider: String,
    repo: String,
    work_id: Option<String>,
    backend: String,
    backend_instance: Option<String>,
    account_label: Option<String>,
    auth_source_label: Option<String>,
    quota_pool: Option<String>,
    requested_model: Option<String>,
    effective_model: Option<String>,
    actual_model: Option<String>,
    model_provider: Option<String>,
    usage_classification: Option<String>,
    day: String,
}

#[derive(Debug, Serialize)]
struct Row {
    #[serde(flatten)]
    identity: Identity,
    dispatch_attempts: u64,
    legacy_dispatches: u64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    actual_cost_usd: Option<f64>,
    estimated_cost_usd: Option<f64>,
    measurements: Measurements,
}

#[derive(Debug, Default, Serialize)]
struct Measurement<T> {
    reported_total: Option<T>,
    measured_observations: u64,
    unmeasured_observations: u64,
}

impl<T: Copy> Measurement<T> {
    fn record(&mut self, reading: Option<T>, add: impl FnOnce(T, T) -> Option<T>) -> Option<T> {
        if let Some(value) = reading {
            self.reported_total = if self.measured_observations == 0 {
                Some(value)
            } else {
                self.reported_total.and_then(|total| add(total, value))
            };
            self.measured_observations += 1;
        } else {
            self.unmeasured_observations += 1;
        }
        if self.unmeasured_observations == 0 {
            self.reported_total
        } else {
            None
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct Measurements {
    input_tokens: Measurement<u64>,
    output_tokens: Measurement<u64>,
    total_tokens: Measurement<u64>,
    cache_read_tokens: Measurement<u64>,
    cache_write_tokens: Measurement<u64>,
    reasoning_tokens: Measurement<u64>,
    actual_cost_usd: Measurement<f64>,
    estimated_cost_usd: Measurement<f64>,
}

impl Row {
    fn new(identity: Identity) -> Self {
        Self {
            identity,
            dispatch_attempts: 0,
            legacy_dispatches: 0,
            input_tokens: Some(0),
            output_tokens: Some(0),
            total_tokens: Some(0),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_tokens: Some(0),
            actual_cost_usd: Some(0.0),
            estimated_cost_usd: Some(0.0),
            measurements: Measurements::default(),
        }
    }

    fn add(&mut self, usage: &LedgerUsage, attempt: bool) {
        if attempt {
            self.dispatch_attempts += 1;
        } else {
            self.legacy_dispatches += 1;
        }
        // Provider fields are independent: neither caches nor reasoning are
        // added to a reported total. Any missing reading invalidates its sum.
        for (total, measurement, reading) in [
            (
                &mut self.input_tokens,
                &mut self.measurements.input_tokens,
                usage.input_tokens,
            ),
            (
                &mut self.output_tokens,
                &mut self.measurements.output_tokens,
                usage.output_tokens,
            ),
            (
                &mut self.total_tokens,
                &mut self.measurements.total_tokens,
                usage.total_tokens,
            ),
            (
                &mut self.cache_read_tokens,
                &mut self.measurements.cache_read_tokens,
                usage.cache_read_tokens,
            ),
            (
                &mut self.cache_write_tokens,
                &mut self.measurements.cache_write_tokens,
                usage.cache_write_tokens,
            ),
            (
                &mut self.reasoning_tokens,
                &mut self.measurements.reasoning_tokens,
                usage.reasoning_tokens,
            ),
        ] {
            *total = measurement.record(reading, u64::checked_add);
        }
        for (total, measurement, reading) in [
            (
                &mut self.actual_cost_usd,
                &mut self.measurements.actual_cost_usd,
                usage.actual_cost_usd,
            ),
            (
                &mut self.estimated_cost_usd,
                &mut self.measurements.estimated_cost_usd,
                usage.estimated_cost_usd,
            ),
        ] {
            *total = measurement.record(
                reading.filter(|value| value.is_finite() && *value >= 0.0),
                |a, b| {
                    let sum = a + b;
                    sum.is_finite().then_some(sum)
                },
            );
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct UsageReport {
    schema_version: u32,
    profile: Option<String>,
    since: String,
    generated_at: String,
    entries: u64,
    excluded_preflight_entries: u64,
    rows: Vec<Row>,
}

pub(super) fn build(cfg: &GahConfig, since: &str, profile: Option<&str>) -> Result<UsageReport> {
    let cutoff = ledger::summary::parse_since(since)?;
    let cutoff_time = OffsetDateTime::parse(&cutoff, &Rfc3339)?;
    let now = crate::clock::now_utc();
    let mut groups: BTreeMap<Identity, Row> = BTreeMap::new();
    let mut entries = 0;
    let mut excluded_preflight_entries = 0;
    ledger::visit_entries(cfg, |entry| {
        if profile.is_some_and(|profile| entry.profile != profile)
            || crate::job_kind::JobKind::parse(&entry.mode).is_err()
        {
            return Ok(());
        }
        let timestamp = OffsetDateTime::parse(&entry.timestamp, &Rfc3339)
            .context("execution usage contains an invalid ledger timestamp")?;
        if timestamp < cutoff_time || timestamp > now {
            return Ok(());
        }
        if proven_unstarted(&entry) {
            excluded_preflight_entries += 1;
            return Ok(());
        }
        entries += 1;
        let day = timestamp.to_offset(UtcOffset::UTC).date().to_string();
        if entry.attempts.is_empty()
            || !entry
                .attempts
                .iter()
                .any(|attempt| ledger::summary::usage_has_observation(&attempt.usage))
        {
            let identity = identity(
                &entry,
                &entry.usage,
                &entry.effective_backend,
                entry.requested_model.clone(),
                entry.effective_model.clone(),
                &day,
            );
            let row = groups
                .entry(identity.clone())
                .or_insert_with(|| Row::new(identity));
            row.add(&entry.usage, false);
            row.dispatch_attempts += entry.attempts.len() as u64;
        } else {
            // Attempt records supersede the dispatch-wide mirrored summary,
            // including attempts whose provider reported no usage at all.
            for attempt in &entry.attempts {
                let route = entry
                    .attempt_routing
                    .iter()
                    .find(|route| route.attempt_number == attempt.attempt_number);
                let requested_model = route
                    .and_then(|route| route.identity.as_ref())
                    .and_then(|identity| identity.requested_model.clone());
                let mut identity = identity(
                    &entry,
                    &attempt.usage,
                    &attempt.backend,
                    requested_model,
                    attempt.effective_model.clone(),
                    &day,
                );
                if identity.backend_instance.is_none() {
                    identity.backend_instance = route.map(|route| route.backend_instance.clone());
                }
                if let Some(route) = route.and_then(|route| route.identity.as_ref()) {
                    if identity.account_label.is_none() {
                        identity.account_label.clone_from(&route.account_label);
                    }
                    if identity.auth_source_label.is_none() {
                        identity
                            .auth_source_label
                            .clone_from(&route.auth_source_label);
                    }
                    if identity.quota_pool.is_none() {
                        identity.quota_pool.clone_from(&route.quota_pool);
                    }
                }
                groups
                    .entry(identity.clone())
                    .or_insert_with(|| Row::new(identity))
                    .add(&attempt.usage, true);
            }
        }
        Ok(())
    })?;
    Ok(UsageReport {
        schema_version: 1,
        profile: profile.map(str::to_owned),
        since: cutoff,
        generated_at: now.format(&Rfc3339)?,
        entries,
        excluded_preflight_entries,
        rows: groups.into_values().collect(),
    })
}

fn identity(
    entry: &LedgerEntry,
    usage: &LedgerUsage,
    backend: &str,
    requested_model: Option<String>,
    effective_model: Option<String>,
    day: &str,
) -> Identity {
    Identity {
        profile: entry.profile.clone(),
        project_provider: entry.provider.clone(),
        repo: entry.repo.clone(),
        work_id: entry.work_id.clone(),
        backend: backend.into(),
        backend_instance: usage.backend_instance.clone(),
        account_label: usage.account_label.clone(),
        auth_source_label: usage.auth_source_label.clone(),
        quota_pool: usage.quota_pool.clone(),
        requested_model,
        effective_model,
        actual_model: usage.actual_model.clone(),
        model_provider: usage.provider.clone(),
        usage_classification: usage.usage_classification.clone(),
        day: day.into(),
    }
}

fn proven_unstarted(entry: &LedgerEntry) -> bool {
    entry.attempts.is_empty()
        && entry.attempt_routing.is_empty()
        && entry.review_verdict.is_none()
        && entry
            .validation_result
            .as_deref()
            .is_none_or(|result| matches!(result, "deferred_capacity" | "claim_lost"))
        && entry.attempts_started == Some(0)
        && entry.attempts_completed == Some(0)
        && entry.backend_exit_code.is_none()
        && entry.usage.usage_unknown_reason == Some(UsageUnknownReason::NoAttemptStarted)
        && entry.usage.usage_source.is_none()
        && [
            entry.usage.input_tokens,
            entry.usage.output_tokens,
            entry.usage.total_tokens,
            entry.usage.cache_read_tokens,
            entry.usage.cache_write_tokens,
            entry.usage.reasoning_tokens,
            entry.usage.requests_count,
        ]
        .iter()
        .all(Option::is_none)
        && entry.usage.actual_cost_usd.is_none()
        && entry.usage.estimated_cost_usd.is_none()
}

#[cfg(test)]
mod tests;
