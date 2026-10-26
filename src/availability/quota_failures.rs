//! Quota failures retain their cause across cooldown probes, until a successful run.
use super::*;
use crate::config::{GahConfig, Profile};
use crate::execution_identity::ExecutionIdentity;

/// Automated quota failures always block ticks, even without a usable provider reset.
pub(super) fn cooldown(
    reason: Reason,
    source: Source,
    until: Option<OffsetDateTime>,
    now: OffsetDateTime,
) -> Option<OffsetDateTime> {
    if reason != Reason::QuotaExhausted || source != Source::BackendError {
        return until;
    }
    Some(
        until
            .unwrap_or(now + time::Duration::minutes(30))
            .max(now + time::Duration::minutes(5)),
    )
}

/// Log paths change per attempt; the provider evidence identifies the cause.
pub(super) fn same_cause(left: &AvailabilityRecord, right: &AvailabilityRecord) -> bool {
    let evidence = |record: &AvailabilityRecord| {
        record.last_error_summary.as_deref().map(|summary| {
            summary
                .split("; confidence=")
                .next()
                .unwrap_or(summary)
                .to_string()
        })
    };
    left.reason == right.reason
        && (right.reason != Reason::QuotaExhausted || evidence(left) == evidence(right))
}

pub(super) fn same_scope(left: &AvailabilityRecord, right: &AvailabilityRecord) -> bool {
    left.backend == right.backend
        && left.backend_instance == right.backend_instance
        && left.model == right.model
        && left.quota_pool == right.quota_pool
}

/// Only a successful run clears a quota streak; unrelated and manual blocks survive.
pub(crate) fn clear_quota_failure(
    path: &Path,
    identity: &ExecutionIdentity,
    now: OffsetDateTime,
) -> Result<()> {
    if !path.try_exists()? {
        return Ok(());
    }
    update_state(path, |state| {
        let prior = state.records.iter().rev().find(|record| {
            record.backend == identity.logical_backend
                && record.backend_instance.as_deref() == Some(&identity.backend_instance)
                && record.model == identity.effective_model
                && record.quota_pool == identity.quota_pool
        });
        if let Some(prior) = prior.filter(|record| {
            record.status == Status::Unavailable
                && record.reason == Reason::QuotaExhausted
                && matches!(record.source, Source::BackendError | Source::Reprobe)
        }) {
            state.records.push(AvailabilityRecord {
                status: Status::Available,
                source: Source::BackendError,
                reason: Reason::Unknown,
                observed_at: now_rfc3339(now),
                unavailable_until: None,
                last_error_summary: None,
                ..prior.clone()
            });
        }
    })?;
    Ok(())
}

/// Called only by the writer that claimed a new quota-cause transition under the state lock.
pub(crate) fn notify_quota_exhausted(
    cfg: &GahConfig,
    profile: &Profile,
    identity: &ExecutionIdentity,
) {
    let reason = format!(
        "{}/{}: quota exhausted; routing paused until cooldown or quota reset",
        identity.backend_instance,
        identity.effective_model.as_deref().unwrap_or("default")
    );
    crate::notifications::notify_event(
        cfg,
        profile,
        crate::notifications::NotifyEvent::HumanRequired {
            reason: &reason,
            reference: None,
            reason_code: Some("quota_exhausted"),
            failure_class: "backend_error",
            failure_stage: None,
            error_summary: None,
            attempt_count: None,
            mr_url: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_quota_cooldown_and_cause_transitions_survive_probes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("availability.json");
        let now = OffsetDateTime::now_utc();
        let first = ExecutionIdentity::legacy_candidate("vibe", Some("glm"), None::<String>);
        let mut other = first.clone();
        other.backend_instance = "other-account".into();
        let fail = |identity: &ExecutionIdentity, cause: &str, at| {
            record_unavailable_for_identity(
                &path,
                identity,
                Reason::QuotaExhausted,
                Source::BackendError,
                None,
                Some(cause.into()),
                at,
            )
            .unwrap()
        };
        assert!(fail(
            &first,
            "budget exhausted; confidence=High; log=first",
            now
        ));
        assert!(
            !availability_for_identity(&path, &first, now + time::Duration::minutes(1))
                .unwrap()
                .eligible
        );
        assert!(
            availability_for_identity(&path, &other, now)
                .unwrap()
                .eligible
        );
        let probe = now + time::Duration::minutes(31);
        assert!(
            availability_for_identity(&path, &first, probe)
                .unwrap()
                .eligible
        );
        assert!(
            !fail(
                &first,
                "budget exhausted; confidence=High; log=retry",
                probe
            ),
            "expiry does not announce recovery"
        );
        assert!(
            fail(&first, "weekly quota exhausted", probe),
            "changed causes announce"
        );
        assert!(
            fail(&other, "weekly quota exhausted", probe),
            "accounts stay separate"
        );
        clear_quota_failure(&path, &first, probe).unwrap();
        assert!(
            availability_for_identity(&path, &first, probe)
                .unwrap()
                .eligible
        );
        assert!(fail(&first, "weekly quota exhausted", probe));
        record_unavailable_for_identity(
            &path,
            &first,
            Reason::ManualDisable,
            Source::Manual,
            None,
            None,
            probe,
        )
        .unwrap();
        clear_quota_failure(&path, &first, probe).unwrap();
        assert_eq!(
            availability_for_identity(&path, &first, probe)
                .unwrap()
                .reason,
            Some(Reason::ManualDisable)
        );
        assert_eq!(
            cooldown(
                Reason::QuotaExhausted,
                Source::BackendError,
                Some(now - time::Duration::minutes(1)),
                now
            ),
            Some(now + time::Duration::minutes(5))
        );
        assert_eq!(
            cooldown(Reason::AuthenticationError, Source::BackendError, None, now),
            None
        );
        record_unavailable_for_identity(
            &path,
            &first,
            Reason::QuotaExhausted,
            Source::Reprobe,
            Some(probe + time::Duration::hours(1)),
            Some("weekly quota exhausted".into()),
            probe,
        )
        .unwrap();
        clear_quota_failure(&path, &first, probe).unwrap();
        assert!(
            fail(&first, "weekly quota exhausted", probe),
            "a successful reprobe resets the cause transition"
        );
    }
}
