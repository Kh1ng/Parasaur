//! Durable worker-side fences. A service observation, not a lost transport
//! reply or process-local flag, must resolve an uncertain start. One locked
//! journal owns each verified job directory; it never stores grant plaintext.
use super::*;
use fs2::FileExt;

/// Read a local caller's private expected binding, using context input limits.
pub fn read_binding(path: &Path) -> Result<DispatchBinding> {
    read_json(&read_blob(path, None, MAX_JSON)?)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartGrant {
    pub run_id: String,
    pub node_id: String,
    pub context_sha256: String,
    pub reservation_id: String,
    pub expires_at: String,
    pub token: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    ReservationIntent,
    Ready,
    StartIntent,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartIntent {
    token_sha256: String,
    expires_at: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    binding: DispatchBinding,
    holder_invocation: String,
    phase: Phase,
    start: Option<StartIntent>,
}

/// Holds the cross-process writer lock until the external operation completes
/// or becomes uncertain. The lock file is never renamed or deleted. Reopening
/// a journal preserves its fences; it is not a retry/reset operation.
pub struct WorkerRun {
    directory: std::path::PathBuf,
    _lock: File,
    journal: Journal,
    persistence_uncertain: bool,
}

impl WorkerRun {
    fn lock(directory: &Path) -> Result<File> {
        private_directory(directory)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(directory.join("worker.lock"))?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o077 == 0
                && metadata.nlink() == 1,
            "Native worker lock must be an owned private regular file"
        );
        file.try_lock_exclusive()
            .context("Native worker operation is already in progress")?;
        Ok(file)
    }

    /// Commit reservation intent before transferring a capacity descriptor.
    /// Existing state is never overwritten, even after an incomplete transfer.
    pub fn reserve(
        directory: &Path,
        binding: &DispatchBinding,
        holder_invocation: &str,
    ) -> Result<Self> {
        let lock = Self::lock(directory)?;
        ensure!(
            fs::symlink_metadata(directory.join("worker.json"))
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "Native worker journal already exists or is unavailable; reconcile it"
        );
        let mut run = Self {
            directory: directory.into(),
            _lock: lock,
            journal: Journal {
                version: 1,
                binding: binding.clone(),
                holder_invocation: holder_invocation.into(),
                phase: Phase::ReservationIntent,
                start: None,
            },
            persistence_uncertain: false,
        };
        run.validate(binding)?;
        run.persist()?;
        Ok(run)
    }

    /// Read only the exact expected job's durable state while excluding any
    /// competing stage, start or recovery writer in another process.
    pub fn open(directory: &Path, binding: &DispatchBinding) -> Result<Self> {
        let lock = Self::lock(directory)?;
        let journal = read_json(&read_blob(&directory.join("worker.json"), None, MAX_JSON)?)?;
        let run = Self {
            directory: directory.into(),
            _lock: lock,
            journal,
            persistence_uncertain: false,
        };
        run.validate(binding)?;
        Ok(run)
    }

    fn validate(&self, binding: &DispatchBinding) -> Result<()> {
        let journal = &self.journal;
        let run = uuid::Uuid::parse_str(&binding.run_id)?;
        ensure!(
            !run.is_nil()
                && run.to_string() == binding.run_id
                && binding.context_sha256.len() == 64
                && binding
                    .context_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
                && journal.version == 1
                && serde_json::to_vec(&journal.binding)? == serde_json::to_vec(binding)?
                && journal.holder_invocation.len() == 32
                && journal
                    .holder_invocation
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
                && journal.holder_invocation.bytes().any(|b| b != b'0')
                && (journal.phase == Phase::StartIntent) == journal.start.is_some(),
            "Native worker journal identity or state is invalid"
        );
        if let Some(start) = &journal.start {
            ensure!(
                start.token_sha256.len() == 64
                    && start.token_sha256.bytes().all(|b| b.is_ascii_hexdigit())
                    && time::OffsetDateTime::parse(
                        &start.expires_at,
                        &time::format_description::well_known::Rfc3339
                    )
                    .is_ok(),
                "Invalid native worker start fence"
            );
        }
        Ok(())
    }

    fn persist(&mut self) -> Result<()> {
        ensure!(
            !self.persistence_uncertain,
            "Native worker persistence is uncertain; reopen and inspect it"
        );
        // Latch before writing: a failed rename/directory sync must never allow
        // a caller to fall back to its assumed previous in-memory phase.
        self.persistence_uncertain = true;
        crate::installer::files::replace(
            &self.directory.join("worker.json"),
            &serde_json::to_vec(&self.journal)?,
            0o600,
        )?;
        File::open(&self.directory)?.sync_all()?;
        self.persistence_uncertain = false;
        Ok(())
    }

    /// Called only after the holder's exact invocation has confirmed storing
    /// the descriptor. The transport must also observe its active/exited state
    /// before offering this reservation to central.
    pub fn ready(&mut self) -> Result<()> {
        ensure!(
            self.journal.phase == Phase::ReservationIntent,
            "Native reservation must be reconciled"
        );
        self.journal.phase = Phase::Ready;
        self.persist()
    }

    /// Fence one offer to launch the verified wrapper, before calling transport.
    /// The callback must consume central's one-use grant and still enforce fresh
    /// attempt/publication authority. This fence itself grants no execution.
    /// Any callback error, timeout or lost reply leaves StartIntent on disk.
    pub fn offer_start(
        &mut self,
        grant: &StartGrant,
        send: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.offer_start_checked(grant, send, super::host::require_ready)
    }

    fn offer_start_checked(
        &mut self,
        grant: &StartGrant,
        send: impl FnOnce() -> Result<()>,
        holder: impl FnOnce(&str, &str) -> Result<()>,
    ) -> Result<()> {
        ensure!(
            !self.persistence_uncertain && self.journal.phase == Phase::Ready,
            "Native start is already offered or unavailable; observe it instead"
        );
        let binding = &self.journal.binding;
        let expires = time::OffsetDateTime::parse(
            &grant.expires_at,
            &time::format_description::well_known::Rfc3339,
        )?;
        ensure!(
            grant.run_id == binding.run_id
                && grant.node_id == binding.node_id
                && grant.context_sha256 == binding.context_sha256
                && grant.reservation_id == self.journal.holder_invocation
                && grant.token.len() == 64
                && grant.token.bytes().all(|b| b.is_ascii_hexdigit())
                && expires > crate::clock::now_utc(),
            "Native start grant does not match this current reservation"
        );
        holder(&binding.run_id, &self.journal.holder_invocation)?;
        self.journal.start = Some(StartIntent {
            token_sha256: format!("{:x}", Sha256::digest(grant.token.as_bytes())),
            expires_at: grant.expires_at.clone(),
        });
        self.journal.phase = Phase::StartIntent;
        self.persist()?;
        send()
    }

    /// Safe diagnostic/recovery view: no token or file-system path is exposed.
    pub fn observation(&self) -> Result<serde_json::Value> {
        ensure!(
            !self.persistence_uncertain,
            "Native worker persistence is uncertain"
        );
        Ok(
            serde_json::json!({"binding": self.journal.binding, "phase": self.journal.phase,
            "holder_invocation": self.journal.holder_invocation, "start_offered": self.journal.start.is_some(),
            "execution_authorized": false}),
        )
    }

    /// Combine durable intent with a fresh kernel-manager observation. Missing,
    /// replaced or unreachable holders stay unknown, never Ready. A start fence
    /// cannot be cleared by the later disappearance of a service.
    pub fn inspect(&self) -> Result<serde_json::Value> {
        let mut value = self.observation()?;
        let held = super::host::require_ready(
            &self.journal.binding.run_id,
            &self.journal.holder_invocation,
        )
        .is_ok();
        // Failed or absent manager evidence cannot establish free capacity.
        value["capacity_held"] = if held {
            true.into()
        } else {
            serde_json::Value::Null
        };
        value["state"] = if held && self.journal.phase == Phase::Ready {
            "ready"
        } else {
            "unknown"
        }
        .into();
        Ok(value)
    }
}

#[cfg(test)]
mod tests;
