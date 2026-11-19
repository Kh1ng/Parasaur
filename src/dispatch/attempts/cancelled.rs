use crate::ledger::{AttemptRecord, FailureClass, FailureStage, LedgerEntry, LedgerUsage};
use crate::routing::RouteDecision;
use crate::runner::RunResult;
use crate::usage_attribution::UsageAttribution;

pub(in crate::dispatch) fn record_operator_cancellation(
    ledger: &mut LedgerEntry,
    route: &RouteDecision,
    model: &str,
    result: &RunResult,
    attempt: u32,
) -> bool {
    if result.exit_code != -2 || !crate::dispatch_cancel::requested() {
        return false;
    }
    let usage = super::attempt_usage(
        &result.log_path,
        result.agy_cli_log_delta.as_deref(),
        UsageAttribution::from_route(route).with_fallback_model(model),
        result.transcript_path.as_deref(),
        None,
    );
    record_cancelled_attempt(
        ledger,
        attempt,
        &route.effective_backend,
        model,
        result.exit_code,
        FailureStage::AgentRun,
        result.duration_secs,
        usage,
        result.agy_version.clone(),
        result.resources.clone(),
    );
    true
}

#[allow(clippy::too_many_arguments)]
pub(in crate::dispatch) fn record_cancelled_attempt(
    ledger: &mut LedgerEntry,
    attempt_number: u32,
    backend: &str,
    model: &str,
    exit_code: i32,
    stage: FailureStage,
    duration_seconds: f64,
    usage: LedgerUsage,
    cli_version: Option<String>,
    resources: crate::ledger::AttemptResourceUsage,
) {
    super::mark_shutdown_cancelled(ledger, stage, Some(exit_code));
    ledger.attempts.push(AttemptRecord {
        attempt_number,
        backend: backend.to_string(),
        effective_model: Some(model.to_string()),
        exit_code: Some(exit_code),
        validation_result: Some(crate::dispatch_cancel::terminal_state().into()),
        failure_class: Some(FailureClass::HarnessError.as_str().into()),
        failure_stage: Some(stage.as_str().into()),
        duration_seconds: Some(duration_seconds),
        diff_path: None,
        checkpoint_branch: None,
        checkpoint_sha: None,
        usage,
        cli_version,
        resources: Some(resources),
    });
}
