//! Wall-clock source for expiry decisions and controller deadlines.
//!
//! `GAH_TEST_NOW` is process-local. Child processes receive it only when a
//! harness explicitly adds it to their command environment.

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
    #[cfg(test)]
    if let Some(clock) = test_clock() {
        return *clock.now.lock().expect("test clock mutex poisoned");
    }
    env_now().unwrap_or_else(OffsetDateTime::now_utc)
}

/// Return the same instant as a Chrono UTC value for legacy claim metadata.
pub fn now_chrono() -> chrono::DateTime<chrono::Utc> {
    let now = now_utc();
    chrono::DateTime::from_timestamp(now.unix_timestamp(), now.nanosecond())
        .expect("UTC clock instant is representable by Chrono")
}

/// Wait for a wall-clock deadline while measuring fallback wait time monotonically.
/// Advancing a test clock wakes the waiter immediately; a frozen clock still
/// reaches the deadline after its original real duration.
pub fn wait_until(deadline: OffsetDateTime, shutdown_requested: impl Fn() -> bool) -> bool {
    let remaining = deadline - now_utc();
    let duration = Duration::try_from(remaining).unwrap_or_default();
    let monotonic_deadline = Instant::now() + duration;
    loop {
        if shutdown_requested() {
            return false;
        }
        if now_utc() >= deadline {
            return true;
        }
        let remaining = monotonic_deadline.saturating_duration_since(Instant::now());
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

/// A wall-clock deadline with a monotonic fallback for real waiting.
#[derive(Clone, Copy)]
pub struct Deadline {
    wall: OffsetDateTime,
    monotonic: Instant,
}

impl Deadline {
    pub fn after(duration: Duration) -> Self {
        let wall_duration = time::Duration::try_from(duration).unwrap_or_default();
        Self {
            wall: now_utc() + wall_duration,
            monotonic: Instant::now() + duration,
        }
    }

    pub fn reached(self) -> bool {
        now_utc() >= self.wall || Instant::now() >= self.monotonic
    }

    pub fn remaining(self) -> Duration {
        let wall = Duration::try_from(self.wall - now_utc()).unwrap_or_default();
        wall.min(self.monotonic.saturating_duration_since(Instant::now()))
    }
}

/// Wait for a duration using a monotonic timeout and wake early on simulated advance.
pub fn wait_for(duration: Duration, shutdown_requested: impl Fn() -> bool) -> bool {
    wait_until(
        now_utc() + time::Duration::try_from(duration).unwrap_or_default(),
        shutdown_requested,
    )
}

fn env_now() -> Option<OffsetDateTime> {
    *ENV_NOW.get_or_init(|| {
        std::env::var("GAH_TEST_NOW")
            .ok()
            .and_then(|value| OffsetDateTime::parse(&value, &Rfc3339).ok())
            .map(|value| value.to_offset(UtcOffset::UTC))
    })
}

// ponytail: roughly 300 other wall-clock reads stay outside these targeted domains: availability, claims and retry resets, worker heartbeats, controller deadlines, and watchdog deadlines.

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
