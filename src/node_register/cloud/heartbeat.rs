//! Renew the cloud lease independently of controller work, pacing, and backoff.

use super::{CloudRegistration, GahConfig, OrchestrationMode, RegisterOptions};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const SHUTDOWN_POLL: Duration = Duration::from_millis(250);

/// Keeps one controller's cloud lease alive until shutdown or drop. Local mode
/// starts no thread. Config reloads update the existing worker without delaying
/// its next heartbeat; switching to local mode stops and joins it.
#[derive(Default)]
pub(crate) struct CloudHeartbeat {
    config: Option<Sender<GahConfig>>,
    worker: Option<JoinHandle<()>>,
}

impl CloudHeartbeat {
    pub(crate) fn refresh(&mut self, cfg: &GahConfig) {
        match cfg.defaults.orchestration_mode {
            OrchestrationMode::Local => self.stop(),
            OrchestrationMode::Hosted | OrchestrationMode::SelfHosted => {
                if let Some(config) = &self.config {
                    let _ = config.send(cfg.clone());
                } else {
                    *self = Self::start_with(
                        cfg.clone(),
                        None,
                        |name| std::env::var(name).ok(),
                        crate::runner::shutdown_requested,
                        HEARTBEAT_INTERVAL,
                    );
                }
            }
        }
    }

    fn start_with(
        mut cfg: GahConfig,
        opts: Option<RegisterOptions>,
        env: impl Fn(&str) -> Option<String> + Send + 'static,
        shutdown_requested: impl Fn() -> bool + Send + 'static,
        interval: Duration,
    ) -> Self {
        let (config, updates) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut cloud = CloudRegistration::default();
            let mut next = Instant::now();
            loop {
                if shutdown_requested() {
                    return;
                }
                if Instant::now() >= next {
                    cloud.sync(&cfg, opts.as_ref(), &env);
                    next = Instant::now() + interval;
                }
                let wait = next
                    .saturating_duration_since(Instant::now())
                    .min(SHUTDOWN_POLL);
                match updates.recv_timeout(wait) {
                    Ok(updated) => cfg = updated,
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Self {
            config: Some(config),
            worker: Some(worker),
        }
    }

    fn stop(&mut self) {
        // Like the issue-claim renewer, disconnect to wake the wait before joining.
        drop(self.config.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for CloudHeartbeat {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests;
