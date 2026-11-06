//! Canonical execution observation boundaries shared by reporting and admission.
use super::{LedgerEntry, UsageUnknownReason};

pub(crate) fn has_attempt_usage(entry: &LedgerEntry) -> bool {
    entry
        .attempts
        .iter()
        .any(|attempt| super::summary::usage_has_observation(&attempt.usage))
}

pub(crate) fn proven_unstarted(entry: &LedgerEntry) -> bool {
    entry.attempts.is_empty()
        && entry.attempt_routing.is_empty()
        && entry.review_verdict.is_none()
        && entry
            .validation_result
            .as_deref()
            .is_none_or(|result| matches!(result, "deferred_capacity" | "claim_lost"))
        && entry.attempts_started == Some(0)
        && entry.attempts_completed == Some(0)
        && entry.backend_exit_code.is_none()
        && entry.usage.usage_unknown_reason == Some(UsageUnknownReason::NoAttemptStarted)
        && entry.usage.usage_source.is_none()
        && [
            entry.usage.input_tokens,
            entry.usage.output_tokens,
            entry.usage.total_tokens,
            entry.usage.cache_read_tokens,
            entry.usage.cache_write_tokens,
            entry.usage.reasoning_tokens,
            entry.usage.requests_count,
        ]
        .iter()
        .all(Option::is_none)
        && entry.usage.actual_cost_usd.is_none()
        && entry.usage.estimated_cost_usd.is_none()
}
