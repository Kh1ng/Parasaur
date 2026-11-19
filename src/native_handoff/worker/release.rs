//! Canonical acknowledgement precedes any physical release. Durable stop intent
//! makes a lost stop reply recoverable without stopping a replacement holder.
use super::*;
use crate::ledger::remote_dispatch::ReceiveAcknowledgement;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReleasePhase {
    Acknowledged,
    StopIntent,
    Released,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    version: u32,
    binding: DispatchBinding,
    holder_invocation: String,
    acknowledgement: ReceiveAcknowledgement,
    phase: ReleasePhase,
    context_history_cleaned: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    allowed: bool,
    acknowledgement: ReceiveAcknowledgement,
}

fn matches_ack(
    binding: &DispatchBinding,
    digest: &str,
    ack: &ReceiveAcknowledgement,
) -> Result<()> {
    ensure!(
        ack.run_id == binding.run_id
            && ack.node_id == binding.node_id
            && ack.context_sha256 == binding.context_sha256
            && ack.profile == binding.profile
            && ack.work_id == binding.work_id
            && ack.receipt_sha256 == digest,
        "Central acknowledgement does not match this exact result"
    );
    Ok(())
}

fn manifest(context: &Path, binding: &DispatchBinding) -> Result<Manifest> {
    private_directory(context)?;
    let bytes = read_blob(&context.join("manifest.json"), None, MAX_JSON)?;
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == binding.context_sha256,
        "Native acknowledgement context changed"
    );
    let manifest: Manifest = read_json(&bytes)?;
    ensure!(
        manifest.version == 2
            && manifest.node_id == binding.node_id
            && manifest.run_id == binding.run_id
            && manifest.profile == binding.profile
            && (1..=MAX_HISTORY_CHUNKS).contains(&manifest.history_chunks),
        "Native acknowledgement manifest scope is invalid"
    );
    Ok(manifest)
}

impl WorkerRun {
    pub(super) fn release_observation(&self) -> Result<Option<serde_json::Value>> {
        let path = self.directory.join("release.json");
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => anyhow::bail!("Native release journal is unavailable"),
            Ok(_) => {}
        }
        let release: Release = read_json(&read_blob(&path, None, MAX_JSON)?)?;
        ensure!(
            release.version == 1
                && serde_json::to_vec(&release.binding)?
                    == serde_json::to_vec(&self.journal.binding)?
                && release.holder_invocation == self.journal.holder_invocation,
            "Native release identity changed"
        );
        matches_ack(
            &self.journal.binding,
            &release.acknowledgement.receipt_sha256,
            &release.acknowledgement,
        )?;
        let confirmed = release.phase == ReleasePhase::Released
            && self.journal.launch.as_ref().is_some_and(|launch| {
                super::super::host::released_capacity(
                    &self.journal.binding,
                    &self.journal.holder_invocation,
                    launch,
                    &self.directory,
                )
                .is_ok()
            });
        Ok(Some(
            serde_json::json!({"phase":release.phase,"capacity_released":confirmed,
            "context_history_cleaned":release.context_history_cleaned}),
        ))
    }

    fn persist_release(&mut self, release: &Release) -> Result<()> {
        ensure!(
            !self.persistence_uncertain,
            "Native release persistence is uncertain; reopen and inspect it"
        );
        self.persistence_uncertain = true;
        crate::installer::files::replace(
            &self.directory.join("release.json"),
            &serde_json::to_vec(release)?,
            0o600,
        )?;
        File::open(&self.directory)?.sync_all()?;
        self.persistence_uncertain = false;
        Ok(())
    }

    /// Verify central's durable receipt, fence a stop, prove the original
    /// holder's kernel lease is free, then reclaim only original context chunks.
    /// Keep working ledger/session evidence; no raw acknowledgement is trusted
    /// merely because a caller supplied it or a service token expired.
    pub fn release_result(
        &mut self,
        context: &Path,
        cfg: &GahConfig,
        token: &str,
    ) -> Result<serde_json::Value> {
        let result = self.collect_result(context)?;
        let digest = result
            .get("receipt_sha256")
            .and_then(serde_json::Value::as_str)
            .context("Native result digest is unavailable")?
            .to_owned();
        drop(result);
        let binding = self.journal.binding.clone();
        let holder = self.journal.holder_invocation.clone();
        let launch = self
            .journal
            .launch
            .clone()
            .context("Native release has no launched identity")?;
        let path = self.directory.join("release.json");
        let mut release: Release = match fs::symlink_metadata(&path) {
            Ok(_) => read_json(&read_blob(&path, None, MAX_JSON)?)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let manifest = manifest(context, &binding)?;
                let origin = super::super::central_url(
                    cfg.defaults
                        .registry_central_url
                        .as_deref()
                        .context("Native worker has no configured central authority")?,
                )?;
                ensure!(
                    origin == manifest.central_url,
                    "Native acknowledgement authority differs from its context"
                );
                let body = serde_json::json!({"run_id":binding.run_id,"node_id":binding.node_id,
                    "context_sha256":binding.context_sha256,"reservation_id":holder,"receipt_sha256":digest});
                let response = crate::curl_http::request_control(
                    &format!("{origin}/api/native-dispatch/acknowledgement"),
                    &serde_json::to_string(&body)?,
                    token,
                )?;
                let reply: Reply = read_json(&super::operations::response_body(response)?)?;
                ensure!(reply.allowed, "Central has not acknowledged this result");
                matches_ack(&binding, &digest, &reply.acknowledgement)?;
                let release = Release {
                    version: 1,
                    binding: binding.clone(),
                    holder_invocation: holder.clone(),
                    acknowledgement: reply.acknowledgement,
                    phase: ReleasePhase::Acknowledged,
                    context_history_cleaned: false,
                };
                self.persist_release(&release)?;
                release
            }
            Err(_) => anyhow::bail!("Native release journal is unavailable"),
        };
        ensure!(
            release.version == 1
                && serde_json::to_vec(&release.binding)? == serde_json::to_vec(&binding)?
                && release.holder_invocation == holder,
            "Native release identity changed"
        );
        matches_ack(&binding, &digest, &release.acknowledgement)?;
        // Cold replay must complete a previous uncertain durability barrier,
        // including when Released was renamed before directory sync failed.
        open_blob(&path, MAX_JSON)?.sync_all()?;
        File::open(&self.directory)?.sync_all()?;
        if release.phase == ReleasePhase::Acknowledged {
            release.phase = ReleasePhase::StopIntent;
            self.persist_release(&release)?;
        }
        let verifier =
            super::super::host::release_capacity(&binding, &holder, &launch, &self.directory)?;
        if release.phase != ReleasePhase::Released {
            release.phase = ReleasePhase::Released;
            self.persist_release(&release)?;
        }
        drop(verifier);
        if !release.context_history_cleaned && clean_context(context, &binding).is_ok() {
            release.context_history_cleaned = true;
            self.persist_release(&release)?;
        }
        Ok(
            serde_json::json!({"binding":binding,"capacity_released":true,
            "context_history_cleaned":release.context_history_cleaned,"execution_authorized":false,
            "reason":(!release.context_history_cleaned).then_some("history_cleanup_pending")}),
        )
    }
}

fn clean_context(context: &Path, binding: &DispatchBinding) -> Result<()> {
    let manifest = manifest(context, binding)?;
    for index in 0..manifest.history_chunks {
        let name = super::super::history::name(index);
        let blob = manifest
            .files
            .get(&name)
            .context("Native history chunk identity is unavailable")?;
        let path = context.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                copy_blob(&path, Some(blob), MAX_HISTORY, std::io::sink())?;
                fs::remove_file(path)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => anyhow::bail!("Native history cleanup is unavailable"),
        }
    }
    File::open(context)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::remote_dispatch::ReceiveOutcome;

    fn acknowledgement(binding: &DispatchBinding) -> ReceiveAcknowledgement {
        ReceiveAcknowledgement {
            outcome: ReceiveOutcome::Recorded,
            run_id: binding.run_id.clone(),
            node_id: binding.node_id.clone(),
            profile: binding.profile.clone(),
            work_id: binding.work_id.clone(),
            context_sha256: binding.context_sha256.clone(),
            receipt_sha256: "e".repeat(64),
        }
    }

    #[test]
    fn acknowledgement_matches_every_scope_field_and_never_accepts_unknown_outcomes() {
        let binding = super::super::tests::binding();
        let ack = acknowledgement(&binding);
        matches_ack(&binding, &"e".repeat(64), &ack).unwrap();
        for field in [
            "run_id",
            "node_id",
            "profile",
            "work_id",
            "context_sha256",
            "receipt_sha256",
        ] {
            let mut value = serde_json::to_value(&ack).unwrap();
            value[field] = "wrong".into();
            let changed = serde_json::from_value(value).unwrap();
            assert!(
                matches_ack(&binding, &"e".repeat(64), &changed).is_err(),
                "accepted changed {field}"
            );
        }
        let mut value = serde_json::to_value(&ack).unwrap();
        value["outcome"] = "unknown".into();
        assert!(serde_json::from_value::<ReceiveAcknowledgement>(value).is_err());
    }

    #[test]
    fn uncertain_release_write_latches_before_any_stop_can_be_offered() {
        let directory = super::super::tests::directory();
        let binding = super::super::tests::binding();
        let mut run = WorkerRun::reserve(directory.path(), &binding, &"a".repeat(32)).unwrap();
        fs::create_dir(directory.path().join("release.json")).unwrap();
        let release = Release {
            version: 1,
            binding: binding.clone(),
            holder_invocation: "a".repeat(32),
            acknowledgement: acknowledgement(&binding),
            phase: ReleasePhase::Acknowledged,
            context_history_cleaned: false,
        };
        assert!(run.persist_release(&release).is_err());
        assert!(run.persistence_uncertain);
        assert!(run.persist_release(&release).is_err());
    }
}
