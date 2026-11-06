//! Short-lived worker operation permissions. The one-entry journal is a fence,
//! not proof of backend completion. Only an explicit, confirmed completion
//! allows another operation; errors and process death leave the fence intact.
use super::*;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationPurpose {
    Attempt,
    Publish,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OperationRequest {
    pub run_id: String,
    pub node_id: String,
    pub context_sha256: String,
    pub reservation_id: String,
    pub sequence: u64,
    pub purpose: OperationPurpose,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationPermission {
    pub allowed: bool,
    pub run_id: String,
    pub node_id: String,
    pub context_sha256: String,
    pub reservation_id: String,
    pub sequence: u64,
    pub purpose: OperationPurpose,
    pub authorized_at: String,
    pub expires_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartConsumption {
    pub allowed: bool,
    pub run_id: String,
    pub node_id: String,
    pub context_sha256: String,
    pub reservation_id: String,
    pub consumed_at: String,
    pub expires_at: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Execution {
    pub(super) invocation: String,
    pub(super) pid: u32,
    pub(super) start_ticks: String,
    consumed_at: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OperationPhase {
    Pending,
    Authorized,
    Completed,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Operation {
    sequence: u64,
    purpose: OperationPurpose,
    phase: OperationPhase,
    permission: Option<OperationPermission>,
}

fn date(value: &str) -> Result<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).context("Invalid native permission timestamp")
}

fn response_body(response: crate::curl_http::CurlResponse) -> Result<Vec<u8>> {
    if response.status == 200 {
        return Ok(response.body);
    }
    let body: serde_json::Value =
        serde_json::from_slice(&response.body).unwrap_or(serde_json::Value::Null);
    let reason = match body.get("reason").and_then(serde_json::Value::as_str) {
        Some(
            code @ ("factory_paused"
            | "policy_changed"
            | "budget_held"
            | "controller_changed"
            | "claim_lost"
            | "authority_busy"
            | "invalid_operation"
            | "operation_in_progress"
            | "worker_membership_unavailable"
            | "invalid_grant"
            | "expired"
            | "already_consumed"
            | "consumption_in_progress"),
        ) if body.get("allowed").and_then(serde_json::Value::as_bool) == Some(false) => code,
        _ => "authority_unavailable",
    };
    anyhow::bail!("Native control held by central ({reason})")
}

impl WorkerRun {
    pub(super) fn operation_observation(&self) -> serde_json::Value {
        self.journal.operation.as_ref().map_or(serde_json::Value::Null,|operation|
            serde_json::json!({"sequence":operation.sequence,"purpose":operation.purpose,"state":operation.phase}))
    }
    pub(super) fn validate_operations(&self) -> Result<()> {
        ensure!(
            !self.journal.consumption_offered || self.journal.phase == Phase::StartIntent,
            "Native consumption has no durable start intent"
        );
        if let Some(execution) = &self.journal.execution {
            ensure!(
                self.journal.phase == Phase::StartIntent
                    && self.journal.consumption_offered
                    && execution.pid > 0
                    && execution.invocation.len() == 32
                    && execution.invocation.bytes().all(|b| b.is_ascii_hexdigit())
                    && execution.invocation.bytes().any(|b| b != b'0')
                    && execution.start_ticks.parse::<u64>().is_ok_and(|v| v > 0),
                "Invalid native execution identity"
            );
            let consumed = date(&execution.consumed_at)?;
            let expires = date(
                &self
                    .journal
                    .start
                    .as_ref()
                    .context("Missing start fence")?
                    .expires_at,
            )?;
            ensure!(
                consumed < expires && expires - consumed <= time::Duration::seconds(10),
                "Invalid consumed native start"
            );
        }
        if let Some(operation) = &self.journal.operation {
            ensure!(
                self.journal.execution.is_some()
                    && (1..=9_007_199_254_740_991).contains(&operation.sequence)
                    && (operation.phase == OperationPhase::Pending)
                        == operation.permission.is_none(),
                "Invalid native operation fence"
            );
            if let Some(permission) = &operation.permission {
                self.validate_permission(
                    permission,
                    &self.operation_request(operation.sequence, operation.purpose),
                    false,
                )?;
            }
        }
        Ok(())
    }

    /// Enter once, after the fixed wrapper receives central's consumed-start
    /// response. A restart/re-exec never inherits entry through old files.
    pub fn enter(&mut self, consumed: &StartConsumption) -> Result<()> {
        ensure!(
            !self.persistence_uncertain
                && self.journal.phase == Phase::StartIntent
                && self.journal.execution.is_none(),
            "Native execution already entered or lacks a durable start fence"
        );
        let binding = &self.journal.binding;
        ensure!(
            consumed.allowed
                && consumed.run_id == binding.run_id
                && consumed.node_id == binding.node_id
                && consumed.context_sha256 == binding.context_sha256
                && consumed.reservation_id == self.journal.holder_invocation
                && self
                    .journal
                    .start
                    .as_ref()
                    .is_some_and(|start| start.expires_at == consumed.expires_at)
                && date(&consumed.consumed_at)? <= crate::clock::now_utc()
                && date(&consumed.expires_at)? > crate::clock::now_utc()
                && date(&consumed.expires_at)? - date(&consumed.consumed_at)?
                    <= time::Duration::seconds(10),
            "Consumed native start does not match this current job"
        );
        let invocation =
            std::env::var("INVOCATION_ID").context("Native execution requires its job service")?;
        let pid = std::process::id();
        let start_ticks = super::super::host::process_start_ticks(pid)?;
        super::super::host::require_ready(&binding.run_id, &self.journal.holder_invocation)?;
        super::super::host::require_own_job(&binding.run_id, &invocation, pid, &start_ticks)?;
        self.journal.execution = Some(Execution {
            invocation,
            pid,
            start_ticks,
            consumed_at: consumed.consumed_at.clone(),
        });
        self.journal.consumption_offered = true;
        self.persist()
    }

    fn central_endpoint(&self, context: &WorkerContext, suffix: &str) -> Result<String> {
        ensure!(
            serde_json::to_vec(&context.binding)? == serde_json::to_vec(&self.journal.binding)?,
            "Native authority context changed"
        );
        let origin = context
            .config
            .defaults
            .registry_central_url
            .as_deref()
            .context("Native worker has no configured central authority")?;
        Ok(format!(
            "{}/api/native-dispatch/{suffix}",
            super::super::central_url(origin)?
        ))
    }

    /// Exchange the initial secret once from the actual fixed job service.
    /// Uncertain delivery leaves consumption intent fenced on disk. No secret
    /// is stored in the journal, command line, environment or diagnostics.
    pub fn consume_start(
        &mut self,
        context: &WorkerContext,
        grant: &StartGrant,
        token: &str,
    ) -> Result<()> {
        ensure!(
            !self.persistence_uncertain
                && !self.journal.consumption_offered
                && self.journal.phase == Phase::StartIntent
                && self.journal.execution.is_none(),
            "Native start consumption is already offered or unavailable"
        );
        let binding = &self.journal.binding;
        let start = self
            .journal
            .start
            .as_ref()
            .context("Missing native start fence")?;
        ensure!(
            grant.run_id == binding.run_id
                && grant.node_id == binding.node_id
                && grant.context_sha256 == binding.context_sha256
                && grant.reservation_id == self.journal.holder_invocation
                && grant.expires_at == start.expires_at
                && format!("{:x}", Sha256::digest(grant.token.as_bytes())) == start.token_sha256
                && date(&grant.expires_at)? > crate::clock::now_utc(),
            "Native start secret does not match the current job"
        );
        let invocation =
            std::env::var("INVOCATION_ID").context("Native execution requires its job service")?;
        let pid = std::process::id();
        let ticks = super::super::host::process_start_ticks(pid)?;
        super::super::host::require_ready(&binding.run_id, &self.journal.holder_invocation)?;
        super::super::host::require_own_job(&binding.run_id, &invocation, pid, &ticks)?;
        let endpoint = self.central_endpoint(context, "consume")?;
        self.journal.consumption_offered = true;
        self.persist()?;
        let response =
            crate::curl_http::request_control(&endpoint, &serde_json::to_string(grant)?, token)?;
        let consumed = read_json(&response_body(response)?)?;
        self.enter(&consumed)
    }

    /// Use only the authority origin in the already verified worker context.
    /// Denials and malformed/oversized replies leave an unconfirmed operation;
    /// callers cannot substitute another central or replay the same sequence.
    pub fn authorize_central(
        &mut self,
        context: &WorkerContext,
        purpose: OperationPurpose,
        token: &str,
    ) -> Result<OperationPermission> {
        let endpoint = self.central_endpoint(context, "operation")?;
        self.authorize_operation(purpose, |request| {
            let response = crate::curl_http::request_control(
                &endpoint,
                &serde_json::to_string(request)?,
                token,
            )?;
            read_json(&response_body(response)?)
        })
    }

    fn operation_request(&self, sequence: u64, purpose: OperationPurpose) -> OperationRequest {
        let binding = &self.journal.binding;
        OperationRequest {
            run_id: binding.run_id.clone(),
            node_id: binding.node_id.clone(),
            context_sha256: binding.context_sha256.clone(),
            reservation_id: self.journal.holder_invocation.clone(),
            sequence,
            purpose,
        }
    }

    fn require_execution(&self) -> Result<()> {
        let execution = self
            .journal
            .execution
            .as_ref()
            .context("Native job has not consumed start authority")?;
        let binding = &self.journal.binding;
        super::super::host::require_ready(&binding.run_id, &self.journal.holder_invocation)?;
        super::super::host::require_own_job(
            &binding.run_id,
            &execution.invocation,
            execution.pid,
            &execution.start_ticks,
        )
    }

    fn validate_permission(
        &self,
        permission: &OperationPermission,
        request: &OperationRequest,
        fresh: bool,
    ) -> Result<()> {
        let issued = date(&permission.authorized_at)?;
        let expires = date(&permission.expires_at)?;
        ensure!(
            permission.allowed
                && permission.run_id == request.run_id
                && permission.node_id == request.node_id
                && permission.context_sha256 == request.context_sha256
                && permission.reservation_id == request.reservation_id
                && permission.sequence == request.sequence
                && permission.purpose == request.purpose
                && expires > issued
                && expires - issued <= time::Duration::seconds(10),
            "Native operation permission is invalid or has a different scope"
        );
        ensure!(
            issued
                >= date(
                    &self
                        .journal
                        .execution
                        .as_ref()
                        .context("Native execution has not entered")?
                        .consumed_at
                )?,
            "Native operation permission predates start consumption"
        );
        ensure!(
            !fresh || (issued <= crate::clock::now_utc() && expires > crate::clock::now_utc()),
            "Native operation permission is expired or not yet valid"
        );
        Ok(())
    }

    /// Record an operation before HTTP, then confirm both its reply and the
    /// physical job/holder again before recording permission. A failed/lost
    /// request or uncertain write cannot be retried through this journal.
    /// Caller must start before expiry and explicitly complete only after it
    /// proves the backend exited or the publication transaction finished.
    pub fn authorize_operation(
        &mut self,
        purpose: OperationPurpose,
        request: impl FnOnce(&OperationRequest) -> Result<OperationPermission>,
    ) -> Result<OperationPermission> {
        self.authorize_checked(purpose, request, Self::require_execution)
    }

    fn authorize_checked(
        &mut self,
        purpose: OperationPurpose,
        send: impl FnOnce(&OperationRequest) -> Result<OperationPermission>,
        check: impl Fn(&Self) -> Result<()>,
    ) -> Result<OperationPermission> {
        ensure!(
            !self.persistence_uncertain
                && self.journal.phase == Phase::StartIntent
                && self.journal.execution.is_some(),
            "Native execution is unavailable"
        );
        ensure!(
            self.journal
                .operation
                .as_ref()
                .is_none_or(|operation| operation.phase == OperationPhase::Completed),
            "Native operation is still pending or unconfirmed"
        );
        check(self)?;
        let sequence = self
            .journal
            .operation
            .as_ref()
            .map_or(1, |operation| operation.sequence + 1);
        ensure!(
            sequence <= 9_007_199_254_740_991,
            "Native operation sequence is exhausted"
        );
        let request = self.operation_request(sequence, purpose);
        self.journal.operation = Some(Operation {
            sequence,
            purpose,
            phase: OperationPhase::Pending,
            permission: None,
        });
        self.persist()?;
        let permission = send(&request)?;
        self.validate_permission(&permission, &request, true)?;
        check(self)?;
        self.journal.operation = Some(Operation {
            sequence,
            purpose,
            phase: OperationPhase::Authorized,
            permission: Some(permission.clone()),
        });
        self.persist()?;
        self.validate_permission(&permission, &request, true)?;
        Ok(permission)
    }

    /// No Drop completion: an exception, unwinding, timeout or dying wrapper
    /// is not evidence that its agent descendants have stopped.
    pub fn complete_operation(&mut self, permission: &OperationPermission) -> Result<()> {
        self.complete_checked(permission, Self::require_execution)
    }

    fn complete_checked(
        &mut self,
        permission: &OperationPermission,
        check: impl FnOnce(&Self) -> Result<()>,
    ) -> Result<()> {
        ensure!(
            !self.persistence_uncertain,
            "Native worker persistence is uncertain"
        );
        check(self)?;
        let operation = self
            .journal
            .operation
            .as_ref()
            .context("Missing native operation fence")?;
        ensure!(
            operation.phase == OperationPhase::Authorized,
            "Native operation is not authorized or already completed"
        );
        let expected = operation
            .permission
            .as_ref()
            .context("Missing native operation permission")?;
        ensure!(
            serde_json::to_vec(expected)? == serde_json::to_vec(permission)?,
            "Native operation completion scope changed"
        );
        self.journal.operation.as_mut().unwrap().phase = OperationPhase::Completed;
        self.persist()
    }
}

#[cfg(test)]
mod tests;
