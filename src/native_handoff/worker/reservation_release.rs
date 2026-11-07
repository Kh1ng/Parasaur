//! Retire an unused reservation without inventing a completed dispatch. Once
//! stop intent exists, this run can never accept a start, even after restart.
use super::*;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReleasePhase {
    StopIntent,
    Released,
}

#[cfg(test)]
mod tests {
    use super::super::tests::{binding, directory, grant};
    use super::*;

    fn intent(binding: &DispatchBinding) -> Release {
        Release {
            version: 1,
            binding: binding.clone(),
            holder_invocation: "a".repeat(32),
            phase: ReleasePhase::StopIntent,
        }
    }

    #[test]
    fn durable_cancellation_fences_start_after_reload_even_without_a_stop_reply() {
        let directory = directory();
        let binding = binding();
        let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
        run.ready().unwrap();
        run.persist_reservation_release(&intent(&binding)).unwrap();
        assert!(run
            .offer_start_checked(
                &grant(&binding),
                || panic!("started cancelled run"),
                |_, _| panic!("checked a cancelled holder")
            )
            .is_err());
        drop(run);
        let mut cold = WorkerRun::open(directory.path(), &binding).unwrap();
        assert!(cold.require_uncancelled().is_err());
        assert!(cold
            .offer_start_checked(
                &grant(&binding),
                || panic!("replayed cancelled start"),
                |_, _| Ok(())
            )
            .is_err());
        assert!(cold.journal.start.is_none());
    }

    #[test]
    fn wrong_holder_or_any_start_intent_cannot_release_a_reservation() {
        let directory = directory();
        let binding = binding();
        let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
        run.ready().unwrap();
        assert!(run.release_reservation(&"f".repeat(32)).is_err());
        run.offer_start_checked(
            &grant(&binding),
            || Err(anyhow::anyhow!("lost start reply")),
            |_, _| Ok(()),
        )
        .unwrap_err();
        assert!(run.release_reservation(&"a".repeat(32)).is_err());
        assert!(!directory.path().join("reservation-release.json").exists());
    }

    #[test]
    fn uncertain_cancellation_write_prevents_start_and_stop_until_reload() {
        let directory = directory();
        let binding = binding();
        let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
        run.ready().unwrap();
        fs::create_dir(directory.path().join("reservation-release.json")).unwrap();
        assert!(run.persist_reservation_release(&intent(&binding)).is_err());
        assert!(run.persistence_uncertain);
        assert!(run.release_reservation(&"a".repeat(32)).is_err());
        assert!(run
            .offer_start_checked(
                &grant(&binding),
                || panic!("started uncertain run"),
                |_, _| Ok(())
            )
            .is_err());
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    version: u32,
    binding: DispatchBinding,
    holder_invocation: String,
    phase: ReleasePhase,
}

impl WorkerRun {
    pub(super) fn require_uncancelled(&self) -> Result<()> {
        match fs::symlink_metadata(self.directory.join("reservation-release.json")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => anyhow::bail!("Native reservation release was offered; this run cannot start"),
        }
    }

    fn require_unstarted(&self) -> Result<()> {
        ensure!(
            self.journal.phase == Phase::Ready
                && self.journal.start.is_none()
                && self.journal.launch.is_none()
                && self.journal.execution.is_none()
                && self.journal.operation.is_none()
                && !self.journal.consumption_offered,
            "Native start may have been offered; retain capacity for result reconciliation"
        );
        Ok(())
    }

    fn read_reservation_release(&self) -> Result<Release> {
        let release: Release = read_json(&read_blob(
            &self.directory.join("reservation-release.json"),
            None,
            MAX_JSON,
        )?)?;
        ensure!(
            release.version == 1
                && serde_json::to_vec(&release.binding)?
                    == serde_json::to_vec(&self.journal.binding)?
                && release.holder_invocation == self.journal.holder_invocation,
            "Native reservation release identity changed"
        );
        self.require_unstarted()?;
        Ok(release)
    }

    fn persist_reservation_release(&mut self, release: &Release) -> Result<()> {
        ensure!(
            !self.persistence_uncertain,
            "Native reservation release persistence is uncertain; reload its journal"
        );
        self.persistence_uncertain = true;
        crate::installer::files::replace(
            &self.directory.join("reservation-release.json"),
            &serde_json::to_vec(release)?,
            0o600,
        )?;
        File::open(&self.directory)?.sync_all()?;
        self.persistence_uncertain = false;
        Ok(())
    }

    pub(super) fn reservation_release_observation(&self) -> Result<Option<serde_json::Value>> {
        match fs::symlink_metadata(self.directory.join("reservation-release.json")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => anyhow::bail!("Native reservation release journal is unavailable"),
            Ok(_) => {}
        }
        let release = self.read_reservation_release()?;
        let confirmed = release.phase == ReleasePhase::Released
            && super::super::host::release_unused_capacity(
                &self.journal.binding,
                &self.journal.holder_invocation,
                &self.directory,
                false,
            )
            .is_ok();
        Ok(Some(
            serde_json::json!({"phase":release.phase,"capacity_released":confirmed}),
        ))
    }

    /// A service credential is unnecessary: this local-only operation cannot
    /// start work, acknowledge a result or stop any possible job. It requires
    /// the exact private binding, original holder and locked unstarted journal.
    pub fn release_reservation(&mut self, reservation_id: &str) -> Result<serde_json::Value> {
        ensure!(
            !self.persistence_uncertain && reservation_id == self.journal.holder_invocation,
            "Native reservation release does not match its original holder"
        );
        self.require_unstarted()?;
        let mut release =
            match fs::symlink_metadata(self.directory.join("reservation-release.json")) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Release {
                    version: 1,
                    binding: self.journal.binding.clone(),
                    holder_invocation: self.journal.holder_invocation.clone(),
                    phase: ReleasePhase::StopIntent,
                },
                Err(_) => anyhow::bail!("Native reservation release journal is unavailable"),
                Ok(_) => self.read_reservation_release()?,
            };
        // Repeat the durability barrier on a cold retry, including when the
        // preceding rename succeeded but its directory sync did not.
        self.persist_reservation_release(&release)?;
        let verifier = super::super::host::release_unused_capacity(
            &self.journal.binding,
            &self.journal.holder_invocation,
            &self.directory,
            release.phase != ReleasePhase::Released,
        )?;
        release.phase = ReleasePhase::Released;
        self.persist_reservation_release(&release)?;
        drop(verifier);
        Ok(serde_json::json!({"binding":self.journal.binding,
            "reservation_id":self.journal.holder_invocation,"capacity_released":true,
            "execution_authorized":false,"dispatch_started":false}))
    }
}
