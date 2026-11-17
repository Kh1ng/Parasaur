//! Recorded factory spend admission. The ledger remains authoritative; no
//! counter is reset or cached across calendar windows or process restarts.
use crate::{
    config::{GahConfig, SpendBudget, SpendCostBasis},
    ledger::{self, LedgerEntry},
};
use anyhow::Result;
use serde::Serialize;
use time::{
    format_description::well_known::Rfc3339, Date, Duration, Month, OffsetDateTime, UtcOffset,
};

#[derive(Clone, Debug, Serialize)]
pub struct SpendBudgetStatus {
    pub cost_basis: SpendCostBasis,
    pub scope: &'static str,
    pub windows: Vec<SpendWindowStatus>,
    pub intake_held: bool,
    pub hold_reason: Option<String>,
    pub retry_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SpendWindowStatus {
    pub window: &'static str,
    pub starts_at: String,
    pub resets_at: String,
    pub limit_usd: f64,
    pub reported_usd: Option<f64>,
    pub measured_observations: u64,
    pub unmeasured_observations: u64,
    pub exhausted: bool,
    pub incomplete: bool,
}

impl SpendBudgetStatus {
    /// A hold is a timed pause, never a permanent work-item human gate.
    pub fn wait_action(&self) -> Option<crate::controller::NextAction> {
        self.hold_reason
            .as_ref()
            .map(|reason| crate::controller::NextAction::WaitUntil {
                until: self.retry_at.clone(),
                reason: format!("spend_budget: {reason}"),
            })
    }
}

/// Evaluate already-observed history without a second ledger read. Unknown
/// costs fail closed only for an explicitly configured budget.
pub fn evaluate(
    budget: Option<&SpendBudget>,
    profile: &str,
    entries: &[LedgerEntry],
    now: OffsetDateTime,
) -> Option<SpendBudgetStatus> {
    let budget = budget.filter(|budget| budget.is_enforced())?;
    let mut status = start(budget, now);
    for entry in entries {
        observe(&mut status, profile, entry, now);
    }
    finish(&mut status, budget, now);
    Some(status)
}

fn start(budget: &SpendBudget, now: OffsetDateTime) -> SpendBudgetStatus {
    let date = now.to_offset(UtcOffset::UTC).date();
    let month_start =
        Date::from_calendar_date(date.year(), date.month(), 1).expect("valid month start");
    let month_end = if date.month() == Month::December {
        Date::from_calendar_date(date.year() + 1, Month::January, 1)
    } else {
        Date::from_calendar_date(date.year(), date.month().next(), 1)
    }
    .expect("next calendar month");
    let windows = [
        (
            "day",
            budget.daily_usd,
            date,
            date.next_day().expect("next calendar day"),
        ),
        ("month", budget.monthly_usd, month_start, month_end),
    ]
    .into_iter()
    .filter_map(|(window, limit, start, end)| {
        limit.map(|limit_usd| SpendWindowStatus {
            window,
            starts_at: start
                .with_hms(0, 0, 0)
                .unwrap()
                .assume_utc()
                .format(&Rfc3339)
                .unwrap(),
            resets_at: end
                .with_hms(0, 0, 0)
                .unwrap()
                .assume_utc()
                .format(&Rfc3339)
                .unwrap(),
            limit_usd,
            reported_usd: Some(0.0),
            measured_observations: 0,
            unmeasured_observations: 0,
            exhausted: false,
            incomplete: false,
        })
    })
    .collect();
    SpendBudgetStatus {
        cost_basis: budget.cost_basis,
        scope: "recorded-factory-ledger",
        windows,
        intake_held: false,
        hold_reason: None,
        retry_at: now.format(&Rfc3339).unwrap(),
    }
}

fn observe(
    status: &mut SpendBudgetStatus,
    profile: &str,
    entry: &LedgerEntry,
    now: OffsetDateTime,
) {
    if entry.profile != profile
        || crate::job_kind::JobKind::parse(&entry.mode).is_err()
        || ledger::execution_usage::proven_unstarted(entry)
    {
        return;
    }
    let timestamp = OffsetDateTime::parse(&entry.timestamp, &Rfc3339).ok();
    let basis = status.cost_basis;
    for window in &mut status.windows {
        let Some(timestamp) = timestamp else {
            window.incomplete = true;
            continue;
        };
        let starts =
            OffsetDateTime::parse(&window.starts_at, &Rfc3339).expect("generated timestamp");
        let ends = OffsetDateTime::parse(&window.resets_at, &Rfc3339).expect("generated timestamp");
        // Future execution records cannot be silently ignored to admit spend.
        if timestamp > now {
            window.incomplete = true;
            continue;
        }
        if timestamp < starts || timestamp >= ends {
            continue;
        }
        let mut add = |usage: &ledger::LedgerUsage| {
            let reading = match basis {
                SpendCostBasis::Actual => usage.actual_cost_usd,
                SpendCostBasis::Estimated => usage.estimated_cost_usd,
            };
            if let Some(value) = reading.filter(|value| value.is_finite() && *value >= 0.0) {
                window.measured_observations += 1;
                window.reported_usd = window.reported_usd.and_then(|sum| {
                    let sum = sum + value;
                    sum.is_finite().then_some(sum)
                });
                if window.reported_usd.is_none() {
                    window.incomplete = true;
                }
            } else {
                window.unmeasured_observations += 1;
            }
        };
        if ledger::execution_usage::has_attempt_usage(entry) {
            for attempt in &entry.attempts {
                add(&attempt.usage);
            }
        } else {
            add(&entry.usage);
        }
    }
}

fn finish(status: &mut SpendBudgetStatus, budget: &SpendBudget, now: OffsetDateTime) {
    if budget.validate().is_err() {
        status.hold_reason = Some("invalid spend budget configuration".into());
    }
    for window in &mut status.windows {
        window.incomplete |= window.unmeasured_observations > 0;
        if window.measured_observations == 0 && window.incomplete {
            window.reported_usd = None;
        }
        // Decimal dollar readings arrive as f64. A handful of rounding ULPs
        // below equality must not authorize another dispatch (e.g. 10 × $0.10).
        window.exhausted = window.reported_usd.is_some_and(|sum| {
            sum >= window.limit_usd
                || window.limit_usd - sum <= 4.0 * f64::EPSILON * sum.max(window.limit_usd)
        });
        if status.hold_reason.is_none() && (window.exhausted || window.incomplete) {
            status.hold_reason = Some(if window.exhausted {
                format!(
                    "{} ceiling of ${:.4} reached; resets {}",
                    window.window, window.limit_usd, window.resets_at
                )
            } else {
                format!("{} cost coverage is incomplete; intake paused until readings can be verified or the window resets {}", window.window, window.resets_at)
            });
        }
    }
    status.intake_held = status.hold_reason.is_some();
    // Recheck on the short cadence and at an earlier calendar rollover.
    // A restarted controller can also load a changed policy before rollover.
    let retry = status
        .windows
        .iter()
        .filter_map(|window| OffsetDateTime::parse(&window.resets_at, &Rfc3339).ok())
        .filter(|reset| *reset > now)
        .fold(now + Duration::seconds(60), std::cmp::min);
    status.retry_at = retry.format(&Rfc3339).unwrap();
}

#[derive(Debug)]
pub struct SpendBudgetHeld(pub SpendBudgetStatus);
impl std::fmt::Display for SpendBudgetHeld {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            output,
            "spend budget holds intake: {}",
            self.0.hold_reason.as_deref().unwrap_or("unavailable")
        )
    }
}
impl std::error::Error for SpendBudgetHeld {}

/// Direct and controller dispatch share this preflight. Streaming avoids a
/// second full ledger allocation, and no provider or worktree is touched.
pub fn ensure_available(cfg: &GahConfig, profile: &str) -> Result<()> {
    let policy = crate::config::get_profile(cfg, profile)?.effective_routing(&cfg.defaults);
    let Some(budget) = policy.spend_budget.filter(SpendBudget::is_enforced) else {
        return Ok(());
    };
    let now = crate::clock::now_utc();
    let mut status = start(&budget, now);
    ledger::visit_entries(cfg, |entry| {
        observe(&mut status, profile, &entry, now);
        Ok(())
    })?;
    finish(&mut status, &budget, now);
    if let Some(action) = status.wait_action() {
        if let Err(error) = record_action(cfg, profile, &action) {
            eprintln!("[gah] spend budget notice failed (swallowed): {error:#}");
        }
        Err(SpendBudgetHeld(status).into())
    } else {
        Ok(())
    }
}

/// The loop calls this for each selection. One atomic receipt per profile
/// avoids waking the operator on every tick; delivery failure remains visible
/// through the existing notification channel policy and failure events.
pub(crate) fn record_action(
    cfg: &GahConfig,
    profile_name: &str,
    action: &crate::controller::NextAction,
) -> Result<()> {
    use anyhow::Context;
    use fs2::FileExt;
    use sha2::{Digest, Sha256};
    use std::{
        fs::{self, OpenOptions},
        io::ErrorKind,
    };
    let reason = match action {
        crate::controller::NextAction::WaitUntil { reason, .. } => {
            reason.strip_prefix("spend_budget: ")
        }
        _ => None,
    };
    let directory = cfg
        .defaults
        .ledger_path()
        .with_extension("spend-budget-notices");
    // No enabled budget and no previous receipt: no new filesystem state.
    if reason.is_none() && !directory.exists() {
        return Ok(());
    }
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!(
        "{:x}.json",
        Sha256::digest(profile_name.as_bytes())
    ));
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("lock"))?;
    lock.lock_exclusive()?;
    let previous = match fs::read(&path) {
        Ok(value) => Some(
            serde_json::from_slice::<String>(&value)
                .context("reading spend budget notice receipt")?,
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if previous.as_deref() == reason {
        return Ok(());
    }
    let Some(reason) = reason else {
        if previous.is_some() {
            fs::remove_file(path)?;
        }
        return Ok(());
    };
    let mut pending = tempfile::NamedTempFile::new_in(&directory)?;
    serde_json::to_writer(&mut pending, reason)?;
    pending.as_file().sync_all()?;
    pending.persist(path)?;
    // Reserve an at-most-once notice before external delivery, matching other
    // native notices. Drop the lock before calling any notification hook.
    drop(lock);
    crate::events::record_with_reason_code(
        cfg,
        crate::events::EventType::SpendBudgetHeld,
        Some(profile_name),
        None,
        reason,
        Some("spend_budget"),
    )?;
    let profile = crate::config::get_profile(cfg, profile_name)?;
    crate::notifications::notify_event(
        cfg,
        profile,
        crate::notifications::NotifyEvent::SpendBudgetHeld {
            profile: profile_name,
            reason,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests;
