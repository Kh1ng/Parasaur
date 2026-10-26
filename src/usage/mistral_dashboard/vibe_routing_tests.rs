use super::*;
use crate::{config::CandidateConfig, config::RoutingPolicy, routing::subscription};

fn reading() -> QuotaObservationRecord {
    let fixture = |name| {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/mistral-dashboard")
                .join(format!("{name}.json")),
        )
        .unwrap()
    };
    refresh_with(
        "synthetic-cookie",
        OffsetDateTime::parse("2026-10-03T00:00:00Z", &Rfc3339).unwrap(),
        |_, endpoint, _| {
            Ok(fixture(match endpoint {
                "usage.costBreakdown" => "costBreakdown",
                "usage.breakdownByModel" => "breakdownByModel",
                "usage.prices" => "prices",
                "billing.budget" => "budget",
                _ => panic!("unexpected endpoint"),
            }))
        },
    )
    .unwrap()
}

#[test]
fn vibe_dashboard_refresh_reaches_explicit_account_pool() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quota.jsonl");
    let record = reading();
    let now = OffsetDateTime::parse("2026-10-03T00:01:00Z", &Rfc3339).unwrap();
    // The owner explicitly binds the execution route to the verified account
    // pool printed by quota refresh; an arbitrary Vibe pool inherits nothing.
    let routing = RoutingPolicy::default();
    let identity = routing.execution_identity_for_candidate(&CandidateConfig {
        backend: "vibe".into(),
        model: Some("mistral-medium-3.5".into()),
        quota_pool: record.quota_pool.clone(),
        included_in_quota: true,
        ..Default::default()
    });
    refresh_and_store_with(&path, || Ok(record.clone()), now).unwrap();
    let stored = std::fs::read(&path).unwrap();
    let records = quota_store::load(&path).unwrap();
    let windows = quota_store::latest_windows_for_identity(&records, &identity);
    assert_eq!(
        windows.len(),
        1,
        "bound Vibe route must see dashboard allowance"
    );
    assert_eq!(windows[0].quota_remaining_percent, Some(40.0));
    let capacity = subscription::capacity(&identity, &records, now);
    assert!(capacity.known_capacity && !capacity.exhausted);
    assert!(capacity.reset_pressure.is_some());
    assert_eq!(
        subscription::min_fresh_remaining_percent(&identity, &records, now),
        Some(40.0)
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        stored,
        "lookup never migrates files"
    );

    let mut exhausted = record;
    exhausted.quota_remaining_percent = Some(0.0);
    exhausted.observed_at = now.format(&Rfc3339).ok();
    exhausted.checked_at = exhausted.observed_at.clone();
    refresh_and_store_with(&path, || Ok(exhausted), now).unwrap();
    let capacity = subscription::capacity(&identity, &quota_store::load(&path).unwrap(), now);
    assert!(capacity.exhausted && !capacity.known_capacity);
    assert_eq!(capacity.reset_pressure, Some(0.0));

    refresh_and_store_with(&path, || bail!("auth_required: session expired"), now).unwrap();
    assert_eq!(
        subscription::capacity(&identity, &quota_store::load(&path).unwrap(), now),
        subscription::SubscriptionCapacity::default()
    );
}

#[test]
fn vibe_dashboard_pool_binding_never_inherits_another_account_or_source() {
    let record = reading();
    let now = OffsetDateTime::parse("2026-10-03T00:01:00Z", &Rfc3339).unwrap();
    let routing = RoutingPolicy::default();
    for (backend, pool) in [
        ("vibe", None),
        ("vibe", Some("vibe-monthly")),
        ("vibe", Some("mistral-dashboard:other-account")),
        ("codex", record.quota_pool.as_deref()),
        ("claude", record.quota_pool.as_deref()),
    ] {
        let identity = routing.execution_identity_for_candidate(&CandidateConfig {
            backend: backend.into(),
            quota_pool: pool.map(str::to_owned),
            ..Default::default()
        });
        assert_eq!(
            subscription::capacity(&identity, std::slice::from_ref(&record), now),
            subscription::SubscriptionCapacity::default(),
            "{backend} / {pool:?}"
        );
    }
    let mut identity = routing.execution_identity_for_candidate(&CandidateConfig {
        backend: "vibe".into(),
        quota_pool: record.quota_pool.clone(),
        ..Default::default()
    });
    identity.explicit_instance = true;
    identity.backend_instance = "vibe-work".into();
    assert!(subscription::capacity(&identity, std::slice::from_ref(&record), now).known_capacity);
    // A named source is authoritative even when an account pool also matches.
    identity.quota_source = Some("mistral-console".into());
    assert!(!subscription::capacity(&identity, &[record], now).known_capacity);
}
