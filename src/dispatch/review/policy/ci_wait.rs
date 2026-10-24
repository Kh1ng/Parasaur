use super::ReviewGateContext;
use crate::models::ReviewVerdict;

/// A failed job alone is a head receipt, never actionable code repair context.
/// Require explicit CI-only findings and control-plane failure confirmation.
pub(super) fn defer(verdict: &mut ReviewVerdict, context: &ReviewGateContext) -> bool {
    let ci_only = context.ci_failed
        && verdict.verdict == "NEEDS_FIX"
        && verdict.actionable_findings.is_empty()
        && !verdict.blocking_findings.is_empty()
        && verdict.blocking_findings.iter().all(|finding| {
            finding
                .strip_prefix("ci:failed:")
                .is_some_and(|job| !job.trim().is_empty())
        });
    if ci_only {
        verdict.safety_gate_reason = Some("ci_failed_waiting_for_push".into());
        verdict.human_required = false;
    }
    ci_only
}
