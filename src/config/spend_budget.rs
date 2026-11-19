use serde::{Deserialize, Serialize};

/// Calendar-UTC ceilings for recorded factory execution. Missing cost readings
/// hold new intake; running dispatches are allowed to finish and record costs.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpendBudget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monthly_usd: Option<f64>,
    #[serde(default)]
    pub cost_basis: SpendCostBasis,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpendCostBasis {
    #[default]
    Actual,
    Estimated,
}

impl SpendBudget {
    pub fn is_enforced(&self) -> bool {
        self.daily_usd.is_some() || self.monthly_usd.is_some()
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, limit) in [
            ("daily_usd", self.daily_usd),
            ("monthly_usd", self.monthly_usd),
        ] {
            if limit.is_some_and(|value| !value.is_finite() || value < 0.0) {
                anyhow::bail!("spend_budget.{name} must be a finite nonnegative dollar amount");
            }
        }
        Ok(())
    }
}
