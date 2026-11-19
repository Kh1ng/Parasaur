//! Bound a blank navigation without depending on its load-finished event.
//! One current attempt is retained; newer navigation invalidates queued work.
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::Url;

const CONTENT_DEADLINE: Duration = Duration::from_secs(15);
const PRESENTATION_GRACE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Loading,
    ProbeQueued,
    AwaitingPresentation,
    RecoveryQueued,
}

struct Pending {
    attempt: u64,
    target: Url,
    deadline: Instant,
    phase: Phase,
}

#[derive(Default)]
struct State {
    sequence: u64,
    pending: Option<Pending>,
    stopped: bool,
}

#[derive(Default)]
pub struct NavigationRecovery(Mutex<State>);

pub enum Action {
    Probe { attempt: u64, target: Url },
    Recover { attempt: u64 },
}

impl NavigationRecovery {
    pub fn begin(&self, target: Url, now: Instant) {
        let mut state = self.0.lock().unwrap();
        if state.stopped {
            return;
        }
        state.sequence = state.sequence.wrapping_add(1);
        state.pending = Some(Pending {
            attempt: state.sequence,
            target,
            deadline: now + CONTENT_DEADLINE,
            phase: Phase::Loading,
        });
    }

    pub fn cancel(&self) {
        self.0.lock().unwrap().pending = None;
    }

    pub fn shutdown(&self) {
        let mut state = self.0.lock().unwrap();
        state.pending = None;
        state.stopped = true;
    }

    pub fn stopped(&self) -> bool {
        self.0.lock().unwrap().stopped
    }

    pub fn due(&self, now: Instant) -> Option<Action> {
        let mut state = self.0.lock().unwrap();
        let pending = state.pending.as_mut()?;
        if now < pending.deadline {
            return None;
        }
        match pending.phase {
            Phase::Loading => {
                pending.phase = Phase::ProbeQueued;
                Some(Action::Probe {
                    attempt: pending.attempt,
                    target: pending.target.clone(),
                })
            }
            Phase::AwaitingPresentation => {
                pending.phase = Phase::RecoveryQueued;
                Some(Action::Recover {
                    attempt: pending.attempt,
                })
            }
            _ => None,
        }
    }

    /// Start the acknowledgement grace period when the UI thread actually
    /// sends the probe, not while it is still queued behind other UI work.
    pub fn probe_sent(&self, attempt: u64, now: Instant) -> bool {
        let mut state = self.0.lock().unwrap();
        let Some(pending) = state.pending.as_mut() else {
            return false;
        };
        if pending.attempt != attempt || pending.phase != Phase::ProbeQueued {
            return false;
        }
        pending.phase = Phase::AwaitingPresentation;
        pending.deadline = now + PRESENTATION_GRACE;
        true
    }

    pub fn presented(&self, attempt: u64, from: &Url) -> bool {
        let mut state = self.0.lock().unwrap();
        let matches = state.pending.as_ref().is_some_and(|pending| {
            pending.attempt == attempt
                && pending.phase == Phase::AwaitingPresentation
                && pending.target.origin() == from.origin()
        });
        if matches {
            state.pending = None;
        }
        matches
    }

    /// Recheck the attempt on the UI thread: a new navigation may have happened
    /// after the timeout was queued. Never replace that newer page.
    pub fn expire(&self, attempt: u64) -> Option<Url> {
        let mut state = self.0.lock().unwrap();
        if !state.pending.as_ref().is_some_and(|pending| {
            pending.attempt == attempt && pending.phase == Phase::RecoveryQueued
        }) {
            return None;
        }
        state.pending.take().map(|pending| pending.target)
    }
}

pub fn parse_attempt(value: &str) -> Option<u64> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

pub fn presentation_probe(target: &Url, attempt: u64) -> String {
    let expected = serde_json::json!({ "origin": target.origin().ascii_serialization(), "attempt": attempt.to_string() });
    format!(
        "(() => {{ {} watchNavigationPresentation({expected}); }})();",
        include_str!("navigation_presented.js")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn central() -> Url {
        "https://central.test:8443/".parse().unwrap()
    }
    fn probe(state: &NavigationRecovery, now: Instant) -> u64 {
        let Some(Action::Probe { attempt, .. }) = state.due(now) else {
            panic!("expected probe")
        };
        assert!(state.probe_sent(attempt, now));
        attempt
    }

    #[test]
    fn an_unfinished_navigation_recovers_once_after_the_content_probe() {
        let state = NavigationRecovery::default();
        let now = Instant::now();
        state.begin(central(), now);
        assert!(state.due(now + Duration::from_secs(14)).is_none());
        let attempt = probe(&state, now + CONTENT_DEADLINE);
        assert!(state
            .due(now + CONTENT_DEADLINE + Duration::from_secs(1))
            .is_none());
        assert!(matches!(
            state.due(now + CONTENT_DEADLINE + PRESENTATION_GRACE),
            Some(Action::Recover { .. })
        ));
        assert_eq!(state.expire(attempt), Some(central()));
        assert!(state.due(now + Duration::from_secs(100)).is_none());
    }

    #[test]
    fn visible_content_preserves_the_page_and_allows_spa_navigation() {
        let state = NavigationRecovery::default();
        let now = Instant::now();
        state.begin(central(), now);
        let attempt = probe(&state, now + CONTENT_DEADLINE);
        assert!(!state.presented(attempt, &"https://central.test/".parse().unwrap()));
        assert!(state.presented(
            attempt,
            &"https://central.test:8443/chat?page=chat#turn"
                .parse()
                .unwrap()
        ));
        assert!(state.due(now + Duration::from_secs(100)).is_none());
    }

    #[test]
    fn new_navigation_and_local_settings_invalidate_queued_work() {
        let state = NavigationRecovery::default();
        let now = Instant::now();
        state.begin(central(), now);
        let old = probe(&state, now + CONTENT_DEADLINE);
        assert!(matches!(
            state.due(now + Duration::from_secs(20)),
            Some(Action::Recover { .. })
        ));
        state.begin(central(), now + Duration::from_secs(20));
        assert!(state.expire(old).is_none());
        assert!(!state.presented(old, &central()));
        state.cancel();
        assert!(state.due(now + Duration::from_secs(100)).is_none());
    }

    #[test]
    fn queued_ui_work_gets_its_own_grace_and_cannot_grow_a_timer_history() {
        let state = NavigationRecovery::default();
        let now = Instant::now();
        for _ in 0..1000 {
            state.begin(central(), now);
        }
        let Some(Action::Probe { attempt, .. }) = state.due(now + CONTENT_DEADLINE) else {
            panic!("expected probe")
        };
        assert!(state.due(now + Duration::from_secs(100)).is_none());
        assert!(state.probe_sent(attempt, now + Duration::from_secs(100)));
        assert!(state.due(now + Duration::from_secs(101)).is_none());
        state.shutdown();
        assert!(state.stopped());
        assert!(state.due(now + Duration::from_secs(200)).is_none());
    }

    #[test]
    fn acknowledgement_and_probe_keep_a_bounded_public_contract() {
        assert_eq!(parse_attempt("12"), Some(12));
        assert_eq!(parse_attempt("18446744073709551615"), Some(u64::MAX));
        for value in [
            "",
            "-1",
            "+1",
            "1.0",
            " 12",
            "18446744073709551616",
            "123456789012345678901",
        ] {
            assert!(parse_attempt(value).is_none(), "{value}");
        }
        let script = presentation_probe(
            &"https://central.test:8443/?secret=omit#omit"
                .parse()
                .unwrap(),
            12,
        );
        assert!(!script.contains("secret=omit"));
        assert!(!script.contains("#omit"));
    }
}
