use super::node_capacity;
use crate::controller::NextAction;
use anyhow::Result;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

const REPROBE_INTERVAL: Duration = Duration::from_secs(15);
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(250);

fn interval() -> Duration {
    // Integration tests execute the debug binary, so allow them to shorten
    // the production interval without making the release scheduler tunable
    // through an accidentally inherited environment variable.
    #[cfg(debug_assertions)]
    if let Some(milliseconds) = std::env::var_os("GAH_TEST_NODE_CAPACITY_REPROBE_MS")
        .and_then(|value| value.to_str().and_then(|value| value.parse::<u64>().ok()))
    {
        return Duration::from_millis(milliseconds.clamp(10, 5_000));
    }

    REPROBE_INTERVAL
}

pub(super) enum WaitOutcome<T> {
    WorkerCompleted(T),
    RetryFill,
    KeepWaiting,
    Shutdown,
}

/// Tracks node-deferred actions while other workers remain active.
///
/// Persistent pressure rechecks only local kernel and lease state. Provider
/// state is rebuilt only after a local probe succeeds, immediately before the
/// normal launch path reacquires its fail-closed lease.
#[derive(Default)]
pub(super) struct NodeCapacityReprobe {
    deadline: Option<crate::clock::Deadline>,
    actions: Vec<NextAction>,
}

impl NodeCapacityReprobe {
    pub(super) fn schedule(&mut self, action: NextAction) {
        self.admitted(&action);
        self.deadline = Some(crate::clock::Deadline::after(interval()));
        self.actions.push(action);
    }

    pub(super) fn admitted(&mut self, action: &NextAction) {
        self.actions.retain(|deferred| {
            let same_work = deferred
                .work_id()
                .zip(action.work_id())
                .is_some_and(|(a, b)| {
                    crate::work_claim::normalize_work_identity(a)
                        == crate::work_claim::normalize_work_identity(b)
                });
            !same_work && deferred != action
        });
        if self.actions.is_empty() {
            self.deadline = None;
        }
    }

    /// Release only node deferrals, leaving completed work and route refusals
    /// excluded. Normal launch admission must still acquire its own lease.
    pub(super) fn readmit(&mut self, executed_work_ids: &mut std::collections::HashSet<String>) {
        for action in self.actions.drain(..) {
            if let Some(work_id) = action.work_id() {
                executed_work_ids.remove(&crate::work_claim::normalize_work_identity(work_id));
            }
        }
        self.deadline = None;
    }

    pub(super) fn is_scheduled(&self) -> bool {
        self.deadline.is_some()
    }

    pub(super) fn wait<T>(
        &mut self,
        done_rx: &Receiver<T>,
        active_workers: usize,
        parallel_limit: usize,
        settings: crate::config::NodeCapacitySettings,
    ) -> Result<WaitOutcome<T>> {
        let deadline = self
            .deadline
            .ok_or_else(|| anyhow::anyhow!("node-capacity re-probe has no deadline"))?;
        let wait = deadline.remaining().min(SHUTDOWN_POLL_INTERVAL);

        match done_rx.recv_timeout(wait) {
            // A rejected dispatch also completes a worker. Its completion
            // must not cancel the probe that can make it eligible again.
            Ok(result) => Ok(WaitOutcome::WorkerCompleted(result)),
            Err(RecvTimeoutError::Disconnected) => Err(anyhow::anyhow!(
                "parallel GAH worker channel closed unexpectedly"
            )),
            Err(RecvTimeoutError::Timeout) => {
                if crate::runner::shutdown_requested() {
                    return Ok(WaitOutcome::Shutdown);
                }
                if !deadline.reached() {
                    return Ok(WaitOutcome::KeepWaiting);
                }

                let action = self
                    .actions
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("node-capacity re-probe lost its action"))?;
                match node_capacity::try_acquire(action, active_workers, settings) {
                    Ok(node_capacity::LiveAdmission::Admit(lease)) => {
                        // This is only a readiness probe. Release it before
                        // rebuilding provider state; normal launch admission
                        // reacquires and can still fail closed.
                        drop(lease);
                        Ok(WaitOutcome::RetryFill)
                    }
                    Ok(node_capacity::LiveAdmission::Defer(reason)) => {
                        eprintln!(
                            "gah loop: node capacity remains deferred at {active_workers}/{parallel_limit}: {reason}"
                        );
                        self.deadline = Some(crate::clock::Deadline::after(interval()));
                        Ok(WaitOutcome::KeepWaiting)
                    }
                    Err(error) => {
                        eprintln!("gah loop: node capacity re-probe failed closed: {error}");
                        self.deadline = Some(crate::clock::Deadline::after(interval()));
                        Ok(WaitOutcome::KeepWaiting)
                    }
                }
            }
        }
    }
}
