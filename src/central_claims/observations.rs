//! Claim admission evidence uses the existing event stream, never a second lease store.
use crate::config::{GahConfig, Profile};
use crate::events::{ControllerEvent, EventType};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimOutcome {
    Acquired,
    Refused,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CentralClaimEvidence {
    pub central_url: String,
    pub repo_id: String,
    pub outcome: ClaimOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CentralClaimObservation {
    NotObserved,
    Unavailable,
    Observed {
        outcome: ClaimOutcome,
        observed_at: String,
        work_id: Option<String>,
    },
}

fn central_origin(value: &str) -> Result<String> {
    let url = url::Url::parse(value)?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "central claim evidence requires an origin without credentials"
    );
    Ok(url.origin().ascii_serialization())
}

pub(crate) fn record(
    cfg: &GahConfig,
    profile: &Profile,
    args: &crate::dispatch::DispatchArgs,
    central_url: &str,
    work_id: &str,
    acquired: bool,
) -> Result<()> {
    crate::events::append(
        cfg,
        &ControllerEvent {
            timestamp: crate::clock::now_utc().format(&Rfc3339)?,
            event_type: EventType::CentralClaimAttempt.as_str().into(),
            profile: Some(args.profile.clone()),
            work_id: Some(work_id.into()),
            run_id: args.run_id.clone(),
            details: if acquired {
                "Central claim acquired."
            } else {
                "Central claim refused. Dispatch did not start."
            }
            .into(),
            reason_code: None,
            review_contract_version: Some(crate::ledger::CURRENT_REVIEW_CONTRACT_VERSION),
            remediation_plan: None,
            skipped: Vec::new(),
            central_claim: Some(CentralClaimEvidence {
                central_url: central_origin(central_url)?,
                repo_id: profile.repo_id.clone(),
                outcome: if acquired {
                    ClaimOutcome::Acquired
                } else {
                    ClaimOutcome::Refused
                },
            }),
        },
    )
}

/// Latest matching attempt in append order. This is not a health probe or a live lease.
pub(crate) fn latest(
    cfg: &GahConfig,
    profile_name: &str,
    profile: &Profile,
) -> CentralClaimObservation {
    let Some(origin) = cfg
        .defaults
        .registry_central_url
        .as_deref()
        .and_then(|url| central_origin(url).ok())
    else {
        return CentralClaimObservation::Unavailable;
    };
    let Ok(events) = crate::events::read_events(cfg) else {
        return CentralClaimObservation::Unavailable;
    };
    events
        .into_iter()
        .rev()
        .find_map(|event| {
            let evidence = event.central_claim?;
            (event.event_type == EventType::CentralClaimAttempt.as_str()
                && event.profile.as_deref() == Some(profile_name)
                && evidence.repo_id == profile.repo_id
                && evidence.central_url == origin)
                .then_some(CentralClaimObservation::Observed {
                    outcome: evidence.outcome,
                    observed_at: event.timestamp,
                    work_id: event.work_id,
                })
        })
        .unwrap_or(CentralClaimObservation::NotObserved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_origins_exclude_secrets_and_unknown_outcomes_remain_unknown() {
        assert_eq!(
            central_origin("https://CENTRAL.test:443/").unwrap(),
            "https://central.test"
        );
        for value in [
            "https://user:secret@central.test",
            "https://central.test/path",
            "https://central.test?token=secret",
            "https://central.test#secret",
            "file:///tmp",
        ] {
            assert!(central_origin(value).is_err());
        }
        assert_eq!(
            serde_json::from_str::<ClaimOutcome>("\"future_outcome\"").unwrap(),
            ClaimOutcome::Unknown
        );
    }
}
