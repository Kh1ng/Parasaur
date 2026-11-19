use crate::ledger::{summary::canonical_usage_observations, LedgerEntry};
use serde::Serialize;
use std::collections::BTreeMap;

/// Known costs from canonical ledger usage, not a provider invoice. The selected
/// snapshot profile/time window applies. Missing identities and costs stay unknown.
#[derive(Debug, Clone, Serialize)]
pub struct QuotaAccountSpend {
    pub backend: String,
    pub backend_instance: Option<String>,
    pub observations: usize,
    pub actual_cost_usd: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    /// Counts recorded usage observations, not every attempted execution.
    pub actual_cost_unknown_observations: usize,
}

/// Group canonical usage once, independently of how many routes/models/pools are
/// configured. Never reconstruct account identity from backend or quota names.
pub(super) fn summarize(entries: &[LedgerEntry]) -> Vec<QuotaAccountSpend> {
    let mut accounts = BTreeMap::new();
    for entry in entries {
        for observed in canonical_usage_observations(entry) {
            let backend = crate::config::canonical_backend_name(observed.backend).to_string();
            let instance = observed
                .usage
                .backend_instance
                .clone()
                .filter(|id| !id.is_empty());
            let account = accounts
                .entry((backend.clone(), instance.clone()))
                .or_insert(QuotaAccountSpend {
                    backend,
                    backend_instance: instance,
                    observations: 0,
                    actual_cost_usd: None,
                    estimated_cost_usd: None,
                    actual_cost_unknown_observations: 0,
                });
            account.observations += 1;
            if let Some(cost) = observed
                .usage
                .actual_cost_usd
                .filter(|cost| cost.is_finite() && *cost >= 0.0)
            {
                account.actual_cost_usd = Some(account.actual_cost_usd.unwrap_or(0.0) + cost);
            } else {
                account.actual_cost_unknown_observations += 1;
            }
            if let Some(cost) = observed
                .usage
                .estimated_cost_usd
                .filter(|cost| cost.is_finite() && *cost >= 0.0)
            {
                account.estimated_cost_usd = Some(account.estimated_cost_usd.unwrap_or(0.0) + cost);
            }
        }
    }
    accounts.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{AttemptRecord, LedgerUsage};

    fn entry() -> LedgerEntry {
        LedgerEntry::new(
            "test",
            &crate::config::tests::test_profile_for_notifications(),
            "codex",
            "improve",
            "#1",
            None,
            None,
        )
    }

    fn usage(instance: Option<&str>, actual: Option<f64>, estimate: Option<f64>) -> LedgerUsage {
        LedgerUsage {
            backend_instance: instance.map(str::to_owned),
            actual_cost_usd: actual,
            estimated_cost_usd: estimate,
            ..Default::default()
        }
    }

    #[test]
    fn accounts_reconcile_with_canonical_attempt_cost_without_dispatch_double_counting() {
        let mut dispatch = entry();
        dispatch.usage = usage(Some("work"), Some(5.0), Some(50.0));
        dispatch.attempts = [("work", 2.0, 20.0), ("personal", 3.0, 30.0)]
            .into_iter()
            .map(|(id, cost, estimate)| AttemptRecord {
                backend: "codex".into(),
                usage: usage(Some(id), Some(cost), Some(estimate)),
                ..Default::default()
            })
            .collect();
        let mut legacy = entry();
        legacy.usage = usage(None, Some(40.0), None);
        let entries = [dispatch, legacy];
        let accounts = summarize(&entries);
        assert_eq!(accounts.len(), 3);
        assert_eq!(accounts[0].backend_instance, None);
        assert_eq!(accounts[0].actual_cost_usd, Some(40.0));
        assert_eq!(accounts[1].backend_instance.as_deref(), Some("personal"));
        assert_eq!(accounts[1].actual_cost_usd, Some(3.0));
        assert_eq!(accounts[2].actual_cost_usd, Some(2.0));
        assert_eq!(accounts[2].estimated_cost_usd, Some(20.0));
        let canonical = crate::ledger::summary::build_grouped_summary(
            &entries,
            |_| "all".into(),
            |_| "all".into(),
            |_, _, _| "all".into(),
        )
        .unwrap();
        assert_eq!(
            accounts
                .iter()
                .filter_map(|row| row.actual_cost_usd)
                .sum::<f64>(),
            canonical[0].actual_cost_usd.unwrap()
        );
        assert_eq!(
            accounts
                .iter()
                .filter_map(|row| row.estimated_cost_usd)
                .sum::<f64>(),
            canonical[0].estimated_cost_usd.unwrap()
        );
    }

    #[test]
    fn zero_and_unknown_costs_remain_distinct_from_estimates_and_other_backends() {
        let mut zero = entry();
        zero.usage = usage(Some("work"), Some(0.0), Some(9.0));
        let mut missing = entry();
        missing.usage = usage(Some("work"), None, Some(7.0));
        let mut other = entry();
        other.effective_backend = "claude".into();
        other.usage = usage(Some("work"), Some(6.0), None);
        let accounts = summarize(&[zero, missing, other]);
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].backend, "claude");
        assert_eq!(accounts[0].actual_cost_usd, Some(6.0));
        assert_eq!(accounts[1].actual_cost_usd, Some(0.0));
        assert_eq!(accounts[1].estimated_cost_usd, Some(16.0));
        assert_eq!(accounts[1].observations, 2);
        assert_eq!(accounts[1].actual_cost_unknown_observations, 1);
        assert!(summarize(&[entry()]).is_empty());
        let mut invalid = entry();
        invalid.usage = usage(Some("work"), Some(f64::NAN), Some(f64::INFINITY));
        let mut negative = entry();
        negative.usage = usage(Some("work"), Some(-1.0), Some(-2.0));
        let unknown = summarize(&[invalid, negative]);
        assert_eq!(unknown[0].actual_cost_usd, None);
        assert_eq!(unknown[0].estimated_cost_usd, None);
        assert_eq!(unknown[0].actual_cost_unknown_observations, 2);
    }
}
