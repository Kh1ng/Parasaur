use super::*;

pub(super) fn invalid_review_attempt_chain(
    cfg: &GahConfig,
    profile: &Profile,
    ledger: &LedgerEntry,
    branch: &str,
) -> String {
    let mut attempts = crate::ledger::read_entries(cfg)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| {
            entry.profile == ledger.profile
                && entry.repo_id == profile.repo_id
                && JobKind::parse(&entry.mode) == Ok(JobKind::Review)
                && entry.branch.as_deref() == Some(branch)
                && entry.review_contract_version.unwrap_or(0)
                    >= crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION
                && entry.validation_result.as_deref() == Some("review_output_invalid")
        })
        .map(|entry| {
            let route = route_label(&entry.effective_backend, entry.effective_model.as_deref());
            let reason = entry
                .review_gate_reason
                .as_deref()
                .or(entry.error_summary.as_deref())
                .unwrap_or("unknown invalid-output reason");
            format!("{route} ({})", utf8_safe_prefix(reason, 120))
        })
        .collect::<Vec<_>>();
    if ledger.validation_result.as_deref() == Some("review_output_invalid") {
        let route = route_label(&ledger.effective_backend, ledger.effective_model.as_deref());
        let reason = ledger
            .review_gate_reason
            .as_deref()
            .or(ledger.error_summary.as_deref())
            .unwrap_or("unknown invalid-output reason");
        attempts.push(format!("{route} ({})", utf8_safe_prefix(reason, 120)));
    }
    if attempts.is_empty() {
        "none recorded".to_string()
    } else {
        attempts.join(" -> ")
    }
}
