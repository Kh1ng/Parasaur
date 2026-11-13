//! A review's verdict is copied onto the implementation row it judged, so
//! reports can say which backend wrote code that passed review. That copy is
//! the ledger's one in-place rewrite, and it must happen on the ledger that
//! owns the history.
//!
//! A job the central handed to a worker runs against a staged copy of the
//! central's history and reports back the one row it appended; the worker
//! proves which row is new by checking the history in front of it is
//! untouched. Rewriting an earlier row there made every native review that
//! reached a verdict unreportable: the worker answered "unknown" for ever and
//! the central held the work. So the job leaves its copy alone, and the
//! central attributes the verdict when it imports the review row.

use super::entry::LedgerEntry;
use super::{backfill_review_verdict, ReviewVerdictBackfill};
use crate::config::GahConfig;
use anyhow::Result;

/// Attributes a verdict this process just reached. Returns whether an
/// implementation row was updated.
pub fn attribute_review_verdict(
    cfg: &GahConfig,
    branch: &str,
    backfill: ReviewVerdictBackfill<'_>,
) -> Result<bool> {
    if crate::dispatch::native_guard::active() {
        return Ok(false);
    }
    backfill_review_verdict(cfg, branch, backfill)
}

/// Attributes the verdict carried by a review row a worker sent back.
pub(super) fn attribute_received_review(cfg: &GahConfig, review: &LedgerEntry) -> Result<bool> {
    let (Some(branch), Some(verdict)) =
        (review.branch.as_deref(), review.review_verdict.as_deref())
    else {
        return Ok(false);
    };
    if review.mode != "review" {
        return Ok(false);
    }
    backfill_review_verdict(
        cfg,
        branch,
        ReviewVerdictBackfill {
            verdict,
            confidence: review.review_confidence.as_deref().unwrap_or("unknown"),
            reviewer_backend: review
                .reviewer_backend
                .as_deref()
                .unwrap_or(&review.effective_backend),
            reviewer_model: review
                .reviewer_model
                .as_deref()
                .or(review.effective_model.as_deref()),
            reviewer_tier: review.reviewer_tier.as_deref(),
            review_gate_reason: review.review_gate_reason.as_deref(),
            review_source_sha: review.review_source_sha.as_deref(),
            review_metadata_fingerprint: review.review_metadata_fingerprint.as_deref(),
            review_contract_version: review.review_contract_version,
            review_generation: review.review_generation.as_deref(),
            blocking_findings: &review.review_blocking_findings,
            actionable_findings: &review.review_actionable_findings,
            non_blocking_findings: &review.review_non_blocking_findings,
            risk_notes: &review.review_risk_notes,
            evidence: &review.review_evidence,
            compatibility_evidence: &review.review_compatibility_evidence,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_review_row_from_a_worker_is_attributed_to_the_implementation_it_judged() {
        let (_tmp, cfg) = crate::ledger::test_util::test_config();
        let profile = crate::config::tests::test_profile_for_notifications();
        let row = |mode: &str, backend: &str| {
            let mut entry = LedgerEntry::new("test", &profile, backend, mode, "#6", None, None);
            entry.work_id = Some("#6".into());
            entry.branch = Some("gah/six".into());
            entry.effective_backend = backend.into();
            entry
        };
        crate::ledger::append(&cfg, &row("fix", "codex")).unwrap();

        let mut review = row("review", "claude");
        review.review_verdict = Some("APPROVE".into());
        review.review_confidence = Some("high".into());
        review.review_generation = Some("review-v1:abc".into());
        crate::ledger::append(&cfg, &review).unwrap();
        assert!(attribute_received_review(&cfg, &review).unwrap());

        let rows = crate::ledger::read_entries(&cfg).unwrap();
        assert_eq!(rows.len(), 2, "attribution adds no row");
        assert_eq!(rows[0].review_verdict.as_deref(), Some("APPROVE"));
        assert_eq!(rows[0].reviewer_backend.as_deref(), Some("claude"));
        assert_eq!(rows[0].review_generation.as_deref(), Some("review-v1:abc"));

        // Rows that carry no verdict, or are not reviews, attribute nothing.
        assert!(!attribute_received_review(&cfg, &row("review", "claude")).unwrap());
        let mut fix = row("fix", "codex");
        fix.review_verdict = Some("APPROVE".into());
        assert!(!attribute_received_review(&cfg, &fix).unwrap());
    }
}
