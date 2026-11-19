//! Immutable recovery of actual worker evidence. A finished ledger row is not
//! process-death proof, and a dead process is not proof that an agent succeeded.
use super::*;
use crate::ledger::remote_dispatch::{RemoteExecution, MAX_RECEIPT_BYTES};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spool {
    version: u32,
    binding: DispatchBinding,
    launch: super::super::host::JobIdentity,
    receipt_sha256: String,
    payload_sha256: String,
    receipt_json: String,
}

impl WorkerRun {
    /// Recover once after exact unit termination and empty-cgroup observation.
    /// Replays return the original durable payload, not a reconstructed result.
    /// Nothing here acknowledges central import or releases a reservation.
    pub fn collect_result(&mut self, context: &Path) -> Result<serde_json::Value> {
        ensure!(
            !self.persistence_uncertain,
            "Native worker persistence is uncertain"
        );
        let launch = self
            .journal
            .launch
            .as_ref()
            .context("Native job has no recorded launch")?;
        let path = self.directory.join("result.json");
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                let view = self.result_view(read_json(&read_blob(
                    &path,
                    None,
                    (MAX_RECEIPT_BYTES * 2 + 65536) as u64,
                )?)?)?;
                // Retry the durability barrier too: a previous rename may
                // have succeeded while its directory synchronization failed.
                open_blob(&path, (MAX_RECEIPT_BYTES * 2 + 65536) as u64)?.sync_all()?;
                File::open(&self.directory)?.sync_all()?;
                return Ok(view);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => anyhow::bail!("Native result spool is unavailable"),
        }
        ensure!(
            self.journal.execution.is_some() && self.journal.consumption_offered,
            "Native job has no consumed execution identity; retain it for reconciliation"
        );
        let termination = super::super::host::terminal_job(&self.journal.binding.run_id, launch)?;
        let ledger = actual_entry(context, &self.directory, &self.journal.binding)?;
        ensure!(
            super::super::host::terminal_job(&self.journal.binding.run_id, launch)? == termination,
            "Native job changed while reading its result"
        );
        let execution = RemoteExecution {
            node_id: self.journal.binding.node_id.clone(),
            termination,
        };
        // Keep the validated original row verbatim. Re-serializing telemetry
        // can normalize floats, defaults and future fields before central sees
        // them; it also makes cold receipt hashing depend on normalization.
        let mut receipt_json = serde_json::to_string(&serde_json::json!({
            "context_sha256": self.journal.binding.context_sha256, "execution":execution
        }))?;
        receipt_json.pop();
        receipt_json.push_str(",\"ledger\":");
        receipt_json.push_str(std::str::from_utf8(&ledger)?.trim_end());
        receipt_json.push('}');
        ensure!(
            receipt_json.len() <= MAX_RECEIPT_BYTES,
            "Native result exceeds its transport limit; retain it for reconciliation"
        );
        let spool = Spool {
            version: 1,
            binding: self.journal.binding.clone(),
            launch: launch.clone(),
            receipt_sha256: canonical_digest(&receipt_json)?,
            payload_sha256: format!("{:x}", Sha256::digest(receipt_json.as_bytes())),
            receipt_json,
        };
        let mut file = tempfile::NamedTempFile::new_in(&self.directory)?;
        let spool_bytes = serde_json::to_vec(&spool)?;
        ensure!(
            spool_bytes.len() <= MAX_RECEIPT_BYTES * 2 + 65536,
            "Native result spool exceeds its limit"
        );
        file.write_all(&spool_bytes)?;
        file.as_file().sync_all()?;
        file.persist_noclobber(&path)
            .context("Native result already exists or could not be committed")?;
        File::open(&self.directory)?.sync_all()?;
        self.result_view(spool)
    }

    fn result_view(&self, spool: Spool) -> Result<serde_json::Value> {
        ensure!(
            spool.version == 1
                && serde_json::to_vec(&spool.binding)?
                    == serde_json::to_vec(&self.journal.binding)?
                && serde_json::to_vec(&spool.launch)? == serde_json::to_vec(&self.journal.launch)?
                && format!("{:x}", Sha256::digest(spool.receipt_json.as_bytes()))
                    == spool.payload_sha256
                && canonical_digest(&spool.receipt_json)? == spool.receipt_sha256,
            "Native result spool identity or digest changed"
        );
        let receipt_json = spool.receipt_json;
        ensure!(
            receipt_json.len() <= MAX_RECEIPT_BYTES,
            "Native result exceeds its transport limit"
        );
        Ok(
            serde_json::json!({"state":"terminal", "receipt_json":receipt_json,
            "receipt_sha256":spool.receipt_sha256,"execution_authorized":false}),
        )
    }
}

fn canonical_digest(receipt: &str) -> Result<String> {
    let value: serde_json::Value = read_json(receipt.as_bytes())?;
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
}

fn actual_entry(context: &Path, work: &Path, binding: &DispatchBinding) -> Result<Vec<u8>> {
    private_directory(context)?;
    let bytes = read_blob(&context.join("manifest.json"), None, MAX_JSON)?;
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == binding.context_sha256,
        "Native result context does not match its binding"
    );
    let manifest: Manifest = read_json(&bytes)?;
    ensure!(
        manifest.version == 2
            && manifest.node_id == binding.node_id
            && manifest.run_id == binding.run_id
            && manifest.profile == binding.profile
            && (1..=MAX_HISTORY_CHUNKS).contains(&manifest.history_chunks),
        "Native result manifest scope is invalid"
    );
    let mut file = open_blob(
        &work.join("ledger.jsonl"),
        MAX_HISTORY * (u64::from(MAX_HISTORY_CHUNKS) + 1),
    )?;
    // Skip only the exact cryptographically verified original history. Stream
    // those chunks; never allocate the full history just to find one result.
    for index in 0..manifest.history_chunks {
        let blob = manifest
            .files
            .get(&super::super::history::name(index))
            .context("Native result history is incomplete")?;
        ensure!(
            blob.bytes <= MAX_HISTORY,
            "Native result history chunk exceeds its limit"
        );
        let mut chunk = std::io::Read::by_ref(&mut file).take(blob.bytes);
        let mut digest = Sha256::new();
        let count = std::io::copy(&mut chunk, &mut digest)?;
        ensure!(
            count == blob.bytes && format!("{:x}", digest.finalize()) == blob.sha256,
            "Native result's original history changed"
        );
    }
    let mut reader = BufReader::new(file);
    let mut found = None;
    loop {
        let mut row = Vec::new();
        let size = std::io::Read::by_ref(&mut reader)
            .take(MAX_RECEIPT_BYTES as u64 + 1)
            .read_until(b'\n', &mut row)?;
        if size == 0 {
            break;
        }
        ensure!(
            size <= MAX_RECEIPT_BYTES && row.ends_with(b"\n"),
            "Native result row is oversized or incomplete"
        );
        let entry: crate::ledger::LedgerEntry = read_json(&row)?;
        if entry.session_id.as_deref() != Some(binding.run_id.as_str()) {
            continue;
        }
        ensure!(
            found.is_none(),
            "Native job has multiple terminal ledger results; reconcile it"
        );
        ensure!(
            entry.profile == binding.profile
                && entry.repo_id == binding.repo_id
                && entry.repo == binding.repo
                && entry.work_id.as_deref() == Some(binding.work_id.as_str())
                && entry.remote_execution.is_none(),
            "Native result ledger scope does not match its job"
        );
        found = Some(row);
    }
    found.context("Native job has no actual ledger result; retain it for reconciliation")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_retains_original_row_and_refuses_changed_history_duplicate_or_partial_results() {
        let _guard = crate::test_support::ExecGuard::new();
        let (directory, central, worker, request) = crate::native_handoff::tests::fixture();
        let profile = &central.profiles["test"];
        let mut entry = crate::ledger::LedgerEntry::new(
            "test",
            profile,
            "codex",
            "fix",
            "fixture",
            Some(uuid::Uuid::new_v4().to_string()),
            None,
        );
        let prefix = format!("{}\n", serde_json::to_string(&entry).unwrap());
        let path = central.defaults.ledger_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &prefix).unwrap();
        let context = directory.path().join("context");
        let binding = super::super::super::prepare(
            &central,
            &request,
            "worker",
            "https://central.invalid",
            &context,
        )
        .unwrap();
        let work = directory.path().join("job");
        super::super::super::load(&worker, &context, &binding.context_sha256, "worker", &work)
            .unwrap();
        let prefix = fs::read_to_string(work.join("ledger.jsonl")).unwrap();
        entry.session_id = Some(binding.run_id.clone());
        entry.work_id = Some(binding.work_id.clone());
        let mut original = serde_json::to_string(&entry).unwrap();
        original.pop();
        original.push_str(",\"future_extension\":{\"precise\":0.1234567890123456789}}\n");
        let ledger = work.join("ledger.jsonl");
        fs::write(&ledger, format!("{prefix}{original}")).unwrap();
        assert_eq!(
            actual_entry(&context, &work, &binding).unwrap(),
            original.as_bytes()
        );
        for changed in [
            format!("{prefix}{original}{original}"),
            format!("{prefix}{}", original.trim_end()),
            format!("{}\n{original}", " ".repeat(prefix.len() - 1)),
        ] {
            fs::write(&ledger, changed).unwrap();
            assert!(actual_entry(&context, &work, &binding).is_err());
        }
    }
}
