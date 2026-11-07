//! Central ingestion of a worker's terminal native dispatch. The trusted caller
//! supplies its prepared binding separately from the worker-controlled receipt.
//! This records execution evidence; it does not authorize a launch or publish.
use super::{LedgerEntry, LEDGER_SCHEMA_VERSION};
use crate::config::GahConfig;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_RECEIPT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DispatchBinding {
    pub run_id: String,
    /// Digest of the prepared central policy/history execution context.
    pub context_sha256: String,
    pub node_id: String,
    pub profile: String,
    pub repo_id: String,
    pub repo: String,
    pub work_id: String,
    pub mode: String,
    pub branch: Option<String>,
    pub review_generation: Option<String>,
    pub dispatch_reason: Option<String>,
    pub backend: String,
    pub backend_instance: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    Exited,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProcessTermination {
    pub reason: TerminationReason,
    pub exit_code: Option<u32>,
    pub signal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemoteExecution {
    pub node_id: String,
    pub termination: ProcessTermination,
}

/// The immutable original receipt digest survives later central review backfill.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReceivedExecution {
    pub node_id: String,
    pub termination: ProcessTermination,
    pub context_sha256: String,
    pub receipt_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchReceipt {
    pub context_sha256: String,
    pub execution: RemoteExecution,
    pub ledger: LedgerEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiveOutcome {
    Recorded,
    AlreadyRecorded,
}

/// Proof returned only after canonical append/replay has been synchronized.
/// A coordinator uses this exact run fence to release its remote claim hold.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiveAcknowledgement {
    pub outcome: ReceiveOutcome,
    pub run_id: String,
    pub node_id: String,
    pub profile: String,
    pub work_id: String,
    pub context_sha256: String,
    pub receipt_sha256: String,
}

/// Preserve the worker's actual job outcome and unknown telemetry after checking
/// its exact prepared identity. A successful process exit does not imply that
/// validation passed, a branch published, or a merge happened.
pub fn receive(
    cfg: &GahConfig,
    binding: &DispatchBinding,
    receipt_json: &[u8],
) -> Result<ReceiveOutcome> {
    Ok(receive_acknowledged(cfg, binding, receipt_json)?.outcome)
}

pub fn receive_acknowledged(
    cfg: &GahConfig,
    binding: &DispatchBinding,
    receipt_json: &[u8],
) -> Result<ReceiveAcknowledgement> {
    ensure!(
        receipt_json.len() <= MAX_RECEIPT_BYTES,
        "Native dispatch input exceeds 8 MiB; retain it for reconciliation"
    );
    let invalid = || anyhow::anyhow!("Native dispatch input does not match the supported schema");
    let original: serde_json::Value =
        serde_json::from_slice(receipt_json).map_err(|_| invalid())?;
    let receipt: DispatchReceipt =
        serde_json::from_value(original.clone()).map_err(|_| invalid())?;
    // Hash the source JSON, not a receiver-normalized projection. Fields added
    // by a future receiver must not change an old immutable receipt's identity.
    // Only this digest is retained; the ledger still uses its normal redaction.
    let receipt_sha256 = format!("{:x}", Sha256::digest(serde_json::to_vec(&original)?));
    let profile = crate::config::get_profile(cfg, &binding.profile)?;
    let run_id = uuid::Uuid::parse_str(&binding.run_id)?;
    ensure!(
        !run_id.is_nil() && run_id.to_string() == binding.run_id,
        "Prepared native run identity must be a canonical UUID"
    );
    ensure!(
        binding.context_sha256.len() == 64
            && binding
                .context_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Prepared dispatch context digest is invalid"
    );
    ensure!(
        profile.repo_id == binding.repo_id && profile.repo == binding.repo,
        "Prepared dispatch repository no longer matches the central profile"
    );
    ensure!(
        !binding.work_id.trim().is_empty()
            && !binding.node_id.is_empty()
            && binding.node_id.len() <= 128
            && binding
                .node_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "Prepared dispatch has an invalid work or node identity"
    );
    let kind = crate::job_kind::JobKind::parse(&binding.mode)?;
    let ledger_mode = if kind.family() == crate::job_kind::JobFamily::ImproveLike
        && profile.delivery_mode == crate::config::DeliveryMode::Handoff
    {
        "handoff"
    } else {
        binding.mode.as_str()
    };
    ensure!(
        kind != crate::job_kind::JobKind::Review || binding.review_generation.is_some(),
        "A remote review requires its selected review generation"
    );
    let entry = &receipt.ledger;
    if ledger_mode == "handoff" {
        ensure!(
            !entry.push_attempted
                && !entry.push_succeeded
                && !entry.mr_attempted
                && !entry.mr_created,
            "Worker attempted a remote publication under read-only delivery policy"
        );
    }
    // A failed preflight can terminate before an observed generation, branch
    // or route exists. That is a failed job, not a review of the selected head.
    let mut usage_without_absence_marker = entry.usage.clone();
    usage_without_absence_marker.usage_unknown_reason = None;
    let no_attempt_failure = entry.failure_class.is_some()
        && entry.attempts_started == Some(0)
        && entry.attempts_completed == Some(0)
        && entry.attempts.is_empty()
        && entry.attempt_routing.is_empty()
        && entry.backend_exit_code.is_none()
        && entry.review_verdict.is_none()
        && entry.usage.usage_unknown_reason == Some(super::UsageUnknownReason::NoAttemptStarted)
        && !super::summary::usage_has_observation(&usage_without_absence_marker);
    let stale_source =
        entry.failure_class.as_deref() == Some("stale_source") && entry.review_verdict.is_none();
    let policy_refusal = entry.dispatch_reason.as_deref() == Some("repo_policy_blocked")
        && entry.failure_class.as_deref() == Some("human_blocked")
        && entry.validation_result.as_deref() == Some("policy_blocked")
        && entry.human_required
        && entry.human_required_reason_code.as_deref() == Some("policy_approval");
    ensure!(
        (1..=LEDGER_SCHEMA_VERSION).contains(&entry.schema_version)
            && entry.remote_execution.is_none(),
        "Unsupported or already attributed worker ledger result"
    );
    ensure!(
        receipt.context_sha256 == binding.context_sha256
            && receipt.execution.node_id == binding.node_id
            && entry.session_id.as_deref() == Some(binding.run_id.to_string().as_str())
            && entry.profile == binding.profile
            && entry.repo_id == binding.repo_id
            && entry.repo == binding.repo
            && entry.provider == profile.provider
            && entry.work_id.as_deref() == Some(binding.work_id.as_str())
            && (entry.mode == ledger_mode
                || (entry.mode == binding.mode && entry.failure_class.is_some()))
            && (entry.review_generation == binding.review_generation
                || stale_source
                || (no_attempt_failure && entry.review_generation.is_none()))
            && (binding.review_generation.is_none()
                || (no_attempt_failure && entry.review_generation.is_none())
                || entry.review_contract_version == Some(super::REVIEW_CONTRACT_VERSION))
            && (entry.dispatch_reason == binding.dispatch_reason || policy_refusal)
            && binding
                .branch
                .as_ref()
                .is_none_or(|branch| entry.branch.as_ref() == Some(branch)
                    || stale_source
                    || (no_attempt_failure && entry.branch.is_none())),
        "Worker result does not match the prepared dispatch identity"
    );
    ensure!(
        (binding.backend == "auto" || entry.requested_backend == binding.backend)
            && binding
                .model
                .as_ref()
                .is_none_or(|model| entry.requested_model.as_ref() == Some(model)
                    || (no_attempt_failure && entry.requested_model.is_none())),
        "Worker result does not match the requested backend or model"
    );
    if let Some(account) = &binding.backend_instance {
        ensure!(
            !account.is_empty()
                && entry.attempt_routing.iter().all(|attempt| attempt
                    .identity
                    .as_ref()
                    .is_some_and(|identity| identity.backend_instance == *account))
                && (!entry.attempt_routing.is_empty() || entry.attempts_started == Some(0))
                && entry
                    .usage
                    .backend_instance
                    .as_ref()
                    .is_none_or(|actual| actual == account),
            "Worker result does not establish the requested account identity"
        );
    }
    let termination = &receipt.execution.termination;
    ensure!(
        !(termination.exit_code.is_some() && termination.signal.is_some())
            && termination
                .signal
                .as_ref()
                .is_none_or(|signal| signal.starts_with("SIG")
                    && signal.len() > 3
                    && signal.len() <= 32
                    && signal[3..]
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
            && (termination.exit_code.is_some() || termination.signal.is_some()),
        "Worker process termination is unconfirmed or malformed"
    );
    let mut entry = entry.clone();
    entry.schema_version = LEDGER_SCHEMA_VERSION;
    entry.remote_execution = Some(ReceivedExecution {
        node_id: receipt.execution.node_id.clone(),
        termination: receipt.execution.termination.clone(),
        context_sha256: binding.context_sha256.clone(),
        receipt_sha256: receipt_sha256.clone(),
    });
    let outcome = super::jsonl::append_received_dispatch(cfg, &entry)?;
    Ok(ReceiveAcknowledgement {
        outcome,
        run_id: binding.run_id.clone(),
        node_id: binding.node_id.clone(),
        profile: binding.profile.clone(),
        work_id: binding.work_id.clone(),
        context_sha256: binding.context_sha256.clone(),
        receipt_sha256,
    })
}

#[cfg(test)]
mod tests;
