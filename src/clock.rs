//! Wall-clock source for expiry decisions and controller deadlines.
//!
//! `GAH_TEST_NOW` is process-local. Child processes receive it only when a
//! harness explicitly adds it to their command environment.
//!
//! Remaining host-clock reads outside this seam, excluding test fixtures:
//! - `src/agents/state.rs:69`
//! - `src/agents/state.rs:75`
//! - `src/auth_health.rs:310`
//! - `src/auth_health.rs:580`
//! - `src/cli/commands/diag.rs:51`
//! - `src/cli/commands/manager_log.rs:13`
//! - `src/cli/commands/profile.rs:512`
//! - `src/cli/commands/skills.rs:87`
//! - `src/cli/commands/skills.rs:119`
//! - `src/credentials/quota.rs:74`
//! - `src/credentials/quota.rs:99`
//! - `src/credentials/quota.rs:151`
//! - `src/credentials/quota.rs:157`
//! - `src/dispatch/workflows/pm/plans.rs:150`
//! - `src/dispatch/workflows/pm/plans.rs:250`
//! - `src/dispatch/workflows/pm/plans.rs:306`
//! - `src/dispatch/workflows/pm.rs:300`
//! - `src/doctor.rs:143`
//! - `src/ledger/approvals.rs:124`
//! - `src/ledger/approvals.rs:461`
//! - `src/ledger/jsonl.rs:346`
//! - `src/ledger/paid_route_notify.rs:37`
//! - `src/ledger/reconcile.rs:372`
//! - `src/ledger/reconcile.rs:472`
//! - `src/ledger/summary.rs:1126`
//! - `src/ledger/summary.rs:1188`
//! - `src/notifications.rs:518`
//! - `src/notifications.rs:618`
//! - `src/notifications.rs:774`
//! - `src/quota_store/agy.rs:89`
//! - `src/quota_store/retention.rs:193`
//! - `src/server.rs:91`
//! - `src/sync.rs:393`
//! - `src/telemetry/exporter.rs:233`
//! - `src/telemetry/exporter.rs:235`
//! - `src/telemetry/exporter.rs:284`
//! - `src/telemetry/exporter.rs:286`
//! - `src/telemetry/health.rs:189`
//! - `src/telemetry/health.rs:191`
//! - `src/telemetry/health.rs:354`
//! - `src/telemetry/mod.rs:230`
//! - `src/telemetry/mod.rs:267`
//! - `src/telemetry/records.rs:550`
//! - `src/tui.rs:74`
//! - `src/usage/agy.rs:59`
//! - `src/usage/claude.rs:95`
//! - `src/usage/claude.rs:100`
//! - `src/usage/claude.rs:134`
//! - `src/usage/mistral_dashboard.rs:123`
//! - `src/usage/mistral_dashboard.rs:128`
//! - `src/usage/mistral_dashboard.rs:176`
//! - `src/usage/nous.rs:17`
//! - `src/usage/nous.rs:26`
//! - `src/usage/nous.rs:152`
//! - `src/usage/vibe_admin.rs:411`
//! - `src/usage.rs:641`
//! - `src/usage_attribution.rs:204`
//! - `src/work_claim.rs:656`
//! - `src/work_claim.rs:705`

use std::sync::OnceLock;
#[cfg(test)]
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
#[cfg(test)]
use std::{cell::RefCell, sync::MutexGuard};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

static ENV_NOW: OnceLock<Option<OffsetDateTime>> = OnceLock::new();
#[cfg(test)]
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
struct SimulatedClock {
    now: Mutex<OffsetDateTime>,
    changed: Condvar,
}

#[cfg(test)]
thread_local! {
    static TEST_NOW: RefCell<Option<Arc<SimulatedClock>>> = const { RefCell::new(None) };
}

/// Initialize the test clock before command dispatch starts.
pub fn init() {
    if let Ok(value) = std::env::var("GAH_TEST_NOW") {
        eprintln!("WARNING: GAH_TEST_NOW is set in this gah process; it overrides wall-clock time and can freeze service cooldowns, claims, worker liveness, and controller deadlines. Remove it from production/service environment files.");
        if OffsetDateTime::parse(&value, &Rfc3339).is_err() {
            eprintln!("WARNING: GAH_TEST_NOW is not valid RFC3339; ignoring the override.");
        }
    }
    let _ = env_now();
}

/// Return the configured test instant, or the current UTC time in production.
pub fn now_utc() -> OffsetDateTime {
    override_now().unwrap_or_else(OffsetDateTime::now_utc)
}

/// Return the same instant as a Chrono UTC value for legacy claim metadata.
pub fn now_chrono() -> chrono::DateTime<chrono::Utc> {
    let now = now_utc();
    chrono::DateTime::from_timestamp(now.unix_timestamp(), now.nanosecond())
        .expect("UTC clock instant is representable by Chrono")
}

/// Return simulated time only when an override is installed; never read host UTC.
pub(crate) fn override_now() -> Option<OffsetDateTime> {
    #[cfg(test)]
    if let Some(clock) = test_clock() {
        return Some(*clock.now.lock().expect("test clock mutex poisoned"));
    }
    env_now()
}

/// Wait until an absolute expiry, converting its initial remaining time once.
pub fn wait_until(deadline: OffsetDateTime, shutdown_requested: impl Fn() -> bool) -> bool {
    wait(Deadline::at(deadline), shutdown_requested)
}

fn wait(deadline: Deadline, shutdown_requested: impl Fn() -> bool) -> bool {
    loop {
        if shutdown_requested() {
            return false;
        }
        let remaining = deadline.remaining();
        if remaining.is_zero() {
            return true;
        }
        #[cfg(test)]
        if let Some(clock) = test_clock() {
            let guard = clock.now.lock().expect("test clock mutex poisoned");
            let _ = clock
                .changed
                .wait_timeout(guard, remaining.min(Duration::from_millis(250)))
                .expect("test clock mutex poisoned");
        } else {
            std::thread::sleep(remaining.min(Duration::from_millis(250)));
        }
        #[cfg(not(test))]
        std::thread::sleep(remaining.min(Duration::from_millis(250)));
    }
}

/// A monotonic budget that also expires on simulated advance with an active override.
/// Relative production deadlines never read UTC, even at construction.
#[derive(Debug, Clone, Copy)]
pub struct Deadline {
    wall: Option<OffsetDateTime>,
    monotonic: Instant,
}

impl Deadline {
    pub fn after(duration: Duration) -> Self {
        Self::after_at(duration, Instant::now())
    }

    pub(crate) fn after_at(duration: Duration, now: Instant) -> Self {
        Self {
            wall: override_now()
                .map(|wall| wall + time::Duration::try_from(duration).unwrap_or_default()),
            monotonic: now + duration,
        }
    }

    /// Convert a provider's absolute expiry once, retaining monotonic production time.
    pub(crate) fn at(wall: OffsetDateTime) -> Self {
        let duration = Duration::try_from(wall - now_utc()).unwrap_or_default();
        Self {
            wall: override_now().map(|_| wall),
            monotonic: Instant::now() + duration,
        }
    }

    pub fn reached(self) -> bool {
        self.reached_at(Instant::now())
    }

    pub(crate) fn reached_at(self, now: Instant) -> bool {
        now >= self.monotonic
            || self
                .wall
                .zip(override_now())
                .is_some_and(|(deadline, now)| now >= deadline)
    }

    pub fn remaining(self) -> Duration {
        self.remaining_at(Instant::now())
    }

    pub(crate) fn remaining_at(self, now: Instant) -> Duration {
        let remaining = self.monotonic.saturating_duration_since(now);
        match self.wall.zip(override_now()) {
            Some((deadline, now)) => {
                remaining.min(Duration::try_from(deadline - now).unwrap_or_default())
            }
            None => remaining,
        }
    }

    pub(crate) fn monotonic(self) -> Instant {
        self.monotonic
    }
}

#[cfg(test)]
impl From<Instant> for Deadline {
    fn from(monotonic: Instant) -> Self {
        Self {
            wall: None,
            monotonic,
        }
    }
}

/// Wait for a monotonic duration and wake early on simulated advance.
pub fn wait_for(duration: Duration, shutdown_requested: impl Fn() -> bool) -> bool {
    wait(Deadline::after(duration), shutdown_requested)
}

fn env_now() -> Option<OffsetDateTime> {
    *ENV_NOW.get_or_init(|| {
        std::env::var("GAH_TEST_NOW")
            .ok()
            .and_then(|value| OffsetDateTime::parse(&value, &Rfc3339).ok())
            .map(|value| value.to_offset(UtcOffset::UTC))
    })
}

#[cfg(test)]
pub(crate) struct TestClockGuard {
    _serial: MutexGuard<'static, ()>,
    clock: Arc<SimulatedClock>,
    previous: Option<Arc<SimulatedClock>>,
}

#[cfg(test)]
impl TestClockGuard {
    pub(crate) fn advance(&self, by: time::Duration) {
        TestClockHandle(Arc::clone(&self.clock)).advance(by);
    }

    pub(crate) fn handle(&self) -> TestClockHandle {
        TestClockHandle(Arc::clone(&self.clock))
    }
}

#[cfg(test)]
impl Drop for TestClockGuard {
    fn drop(&mut self) {
        TEST_NOW.with(|current| *current.borrow_mut() = self.previous.take());
    }
}

#[cfg(test)]
pub(crate) fn set_for_test(now: OffsetDateTime) -> TestClockGuard {
    let serial = TEST_LOCK.lock().expect("test clock test lock poisoned");
    let clock = Arc::new(SimulatedClock {
        now: Mutex::new(now),
        changed: Condvar::new(),
    });
    let previous = TEST_NOW.with(|current| current.borrow_mut().replace(Arc::clone(&clock)));
    TestClockGuard {
        _serial: serial,
        clock,
        previous,
    }
}

#[cfg(test)]
pub(crate) fn test_clock_handle() -> Option<TestClockHandle> {
    test_clock().map(TestClockHandle)
}

#[cfg(test)]
fn test_clock() -> Option<Arc<SimulatedClock>> {
    TEST_NOW.with(|current| current.borrow().clone())
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct TestClockHandle(Arc<SimulatedClock>);

#[cfg(test)]
impl TestClockHandle {
    pub(crate) fn advance(&self, by: time::Duration) {
        let mut now = self.0.now.lock().expect("test clock mutex poisoned");
        *now += by;
        self.0.changed.notify_all();
    }

    pub(crate) fn install(&self) -> TestClockScope {
        let previous = TEST_NOW.with(|current| current.borrow_mut().replace(Arc::clone(&self.0)));
        TestClockScope { previous }
    }
}

#[cfg(test)]
pub(crate) struct TestClockScope {
    previous: Option<Arc<SimulatedClock>>,
}

#[cfg(test)]
impl Drop for TestClockScope {
    fn drop(&mut self) {
        TEST_NOW.with(|current| *current.borrow_mut() = self.previous.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_deadline_ignores_utc_jump_without_override() {
        let clock = set_for_test(OffsetDateTime::UNIX_EPOCH);
        let deadline = Deadline::after(Duration::from_secs(60));
        clock.advance(time::Duration::hours(1));
        assert!(deadline.reached());
        // Model removal of the override after UTC jumped, with the same budget.
        let installed = TEST_NOW.with(|current| current.borrow_mut().take());
        assert!(!deadline.reached());
        assert!(deadline.remaining() > Duration::from_secs(59));
        let production = Deadline::after(Duration::from_secs(60));
        assert!(production.wall.is_none());
        assert!(!production.reached());
        assert!(production.reached_at(production.monotonic));
        TEST_NOW.with(|current| *current.borrow_mut() = installed);
    }

    #[test]
    fn test_clock_parses_rfc3339_and_normalizes_to_utc() {
        let parsed = OffsetDateTime::parse("2026-10-26T12:00:00-05:00", &Rfc3339).unwrap();
        assert_eq!(parsed.to_offset(UtcOffset::UTC).hour(), 17);
    }

    #[test]
    fn advancing_clock_wakes_a_deadline_waiter() {
        let start = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        let clock = set_for_test(start);
        let handle = clock.handle();
        let waiter = std::thread::spawn(move || {
            let _scope = handle.install();
            wait_until(start + time::Duration::hours(1), || false)
        });
        clock.advance(time::Duration::hours(1));
        assert!(waiter.join().unwrap());
    }
}
