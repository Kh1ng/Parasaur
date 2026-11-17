use super::*;

#[test]
fn agy_individual_quota_fixture_persists_identity_cooldown_with_eta_ceiling() {
    let tmp = tempfile::tempdir().unwrap();
    let state_path = tmp.path().join("availability.json");
    let identity = crate::execution_identity::ExecutionIdentity::legacy_candidate(
        "agy-second",
        Some("Gemini 3.5"),
        Some("agy-account-2"),
    );
    let before = OffsetDateTime::now_utc();
    let parsed = mark_backend_unavailable_from_output_for_identity_at(
        &state_path,
        &identity,
        include_str!("../../../../tests/fixtures/quota-logs/agy_individual_quota_reached.txt"),
        "/tmp/backend-output.log",
    )
    .unwrap()
    .unwrap();
    let after = OffsetDateTime::now_utc();
    assert_eq!(
        parsed.kind,
        crate::quota_parser::FailureKind::QuotaExhausted
    );
    let state = load_state(&state_path).unwrap();
    assert_eq!(state.records.len(), 1);
    let record = &state.records[0];
    assert_eq!(record.reason, Reason::QuotaExhausted);
    assert_eq!(
        record.backend_instance.as_deref(),
        Some(identity.backend_instance.as_str())
    );
    assert_eq!(record.model, identity.effective_model);
    assert_eq!(record.quota_pool, identity.quota_pool);
    let until =
        OffsetDateTime::parse(record.unavailable_until.as_deref().unwrap(), &Rfc3339).unwrap();
    let ceiling = time::Duration::seconds(crate::availability::UNAVAILABLE_UNTIL_CEILING_SECONDS);
    assert!(until >= before + ceiling - time::Duration::seconds(1));
    assert!(until <= after + ceiling);
    let decision = availability_for_identity(&state_path, &identity, after).unwrap();
    assert!(!decision.eligible);
    assert_eq!(decision.reason, Some(Reason::QuotaExhausted));
    assert!(
        availability_for_identity(&state_path, &identity, until + time::Duration::seconds(1))
            .unwrap()
            .eligible
    );
    let sibling = crate::execution_identity::ExecutionIdentity::legacy_candidate(
        "agy-other",
        Some("Gemini 3.5"),
        Some("agy-account-3"),
    );
    assert!(
        availability_for_identity(&state_path, &sibling, after)
            .unwrap()
            .eligible
    );
}
