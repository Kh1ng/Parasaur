//! A capacity refusal occurs before recording the lease or offering its FD.
//! The run keeps its verified history and binding across later admissions.
use super::*;

impl WorkerRun {
    pub(in crate::native_handoff) fn defer_capacity(&mut self) -> Result<()> {
        ensure!(
            self.journal.phase == Phase::ReservationIntent,
            "Native admission is not pending"
        );
        self.journal.phase = Phase::WaitingCapacity;
        self.persist()
    }

    fn require_deferred(&self) -> Result<()> {
        if self.journal.phase == Phase::ReservationIntent {
            // Older workers did not record a distinct refusal phase. Their
            // private admission log supplies the reason, never authority.
            let parent = self
                .directory
                .parent()
                .context("Missing native run directory")?;
            private_directory(parent)?;
            let bytes = read_blob(&parent.join("admission.stderr"), None, 16 * 1024)?;
            let message = std::str::from_utf8(&bytes)?;
            ensure!(
                message.lines().count() == 1
                    && message.starts_with("Error: Native host admission deferred: "),
                "No confirmed native capacity refusal"
            );
        } else {
            ensure!(
                self.journal.phase == Phase::WaitingCapacity,
                "Native admission was not deferred"
            );
        }
        self.require_uncancelled()?;
        ensure!(
            !self.persistence_uncertain
                && self.journal.start.is_none()
                && self.journal.launch.is_none()
                && self.journal.execution.is_none()
                && self.journal.operation.is_none()
                && self.journal.claim_operation.is_none()
                && !self.journal.consumption_offered
                && fs::symlink_metadata(self.directory.join("capacity.json"))
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "Native reservation or start requires reconciliation"
        );
        Ok(())
    }

    pub(super) fn deferred_admission(&self) -> Result<()> {
        self.require_deferred()?;
        super::super::host::deferred_holder(
            &self.journal.binding.run_id,
            &self.journal.holder_invocation,
            false,
        )
    }

    pub fn rearm_deferred_admission(&mut self) -> Result<()> {
        self.require_deferred()?;
        super::super::host::deferred_holder(
            &self.journal.binding.run_id,
            &self.journal.holder_invocation,
            true,
        )?;
        if self.journal.phase == Phase::ReservationIntent {
            self.defer_capacity()?;
        }
        Ok(())
    }

    /// Called by the new, verified systemd holder with zero stored FDs and
    /// the writer lock. A replacement cannot take over a transferred lease.
    pub(in crate::native_handoff) fn rebind_deferred(&mut self, holder: &str) -> Result<()> {
        ensure!(
            self.journal.phase == Phase::WaitingCapacity,
            "Native admission has not been rearmed"
        );
        self.require_deferred()?;
        ensure!(
            holder != self.journal.holder_invocation,
            "Native admission holder was not replaced"
        );
        self.journal.holder_invocation = holder.to_owned();
        self.journal.phase = Phase::ReservationIntent;
        self.persist()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{binding, directory, grant};
    use super::*;

    #[test]
    fn a_refusal_survives_reload_and_cannot_reuse_an_old_grant_or_replace_a_lease() {
        let root = directory();
        let binding = binding();
        let old = "a".repeat(32);
        let next = "d".repeat(32);
        let mut run = WorkerRun::reserve(root.path(), &binding, &old).unwrap();
        assert!(run.rebind_deferred(&next).is_err());
        run.defer_capacity().unwrap();
        drop(run);
        let mut cold = WorkerRun::open(root.path(), &binding).unwrap();
        assert!(cold.ready().is_err());
        assert!(cold.rebind_deferred(&old).is_err());
        let lease = root.path().join("capacity.json");
        fs::write(&lease, b"incomplete lease record").unwrap();
        assert!(cold.rebind_deferred(&next).is_err());
        fs::remove_file(lease).unwrap();
        cold.rebind_deferred(&next).unwrap();
        cold.ready().unwrap();
        assert!(cold.rebind_deferred(&old).is_err());
        assert!(cold
            .offer_start_checked(
                &grant(&binding),
                || panic!("stale grant started"),
                |_, _| Ok(())
            )
            .is_err());
    }
}
