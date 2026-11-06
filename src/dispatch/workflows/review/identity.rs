use crate::ledger::LedgerEntry;

/// A handed-off review must still name the generation selected by central.
/// Retain a changed observed generation as stale evidence, never as an opinion.
pub(super) fn assert_selected_generation(
    expected: Option<&str>,
    ledger: &mut LedgerEntry,
) -> anyhow::Result<()> {
    if expected.is_some_and(|expected| ledger.review_generation.as_deref() != Some(expected)) {
        ledger.set_failure(
            crate::ledger::FailureClass::StaleSource,
            crate::ledger::FailureStage::Preflight,
        );
        anyhow::bail!("Selected review generation changed before reviewer launch");
    }
    Ok(())
}

pub(super) fn canonicalize_review_ledger_identity(
    ledger: &mut LedgerEntry,
    source_branch: &str,
    mr_url: Option<&str>,
    mr_title: Option<&str>,
) {
    ledger.branch = Some(source_branch.to_string());
    ledger.mr_url = mr_url.map(str::to_string);
    if ledger.work_id.is_none() {
        ledger.work_id = mr_title.and_then(crate::sync::extract_work_id_from_title);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_generation_refuses_stale_or_missing_observations_before_review() {
        let profile = crate::ledger::test_util::profile();
        let mut entry = LedgerEntry::new("test", &profile, "codex", "review", "branch", None, None);
        assert!(assert_selected_generation(Some("selected"), &mut entry).is_err());
        assert_eq!(entry.failure_class.as_deref(), Some("stale_source"));
        assert_eq!(entry.validation_result, None, "validation has not run");
        entry.review_generation = Some("observed-new".into());
        assert!(assert_selected_generation(Some("selected"), &mut entry).is_err());
        assert_eq!(entry.review_generation.as_deref(), Some("observed-new"));
        assert!(assert_selected_generation(Some("observed-new"), &mut entry).is_ok());
        assert!(assert_selected_generation(None, &mut entry).is_ok());
    }

    #[test]
    fn explicit_mr_identity_is_canonicalized_to_provider_branch_url_and_work() {
        let profile = crate::ledger::test_util::profile();
        let mut ledger = LedgerEntry::new("real", &profile, "claude", "review", "mr:7", None, None);
        ledger.branch = Some("mr:7".into());

        canonicalize_review_ledger_identity(
            &mut ledger,
            "feature/review",
            Some("https://gitlab.example.test/owner/repo/-/merge_requests/7"),
            Some("Draft: [GAH] Fix: #701 metadata identity"),
        );

        assert_eq!(ledger.branch.as_deref(), Some("feature/review"));
        assert_eq!(
            ledger.mr_url.as_deref(),
            Some("https://gitlab.example.test/owner/repo/-/merge_requests/7")
        );
        assert_eq!(ledger.work_id.as_deref(), Some("#701"));
    }

    #[test]
    fn explicit_controller_work_identity_is_not_replaced_by_title_inference() {
        let profile = crate::ledger::test_util::profile();
        let mut ledger = LedgerEntry::new("real", &profile, "claude", "review", "mr:7", None, None);
        ledger.work_id = Some("#700".into());

        canonicalize_review_ledger_identity(
            &mut ledger,
            "feature/review",
            Some("https://example.test/mr/7"),
            Some("Draft: [GAH] Fix: #701 metadata identity"),
        );

        assert_eq!(ledger.work_id.as_deref(), Some("#700"));
    }
}
