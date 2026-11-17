use super::*;
#[cfg(target_os = "linux")]
#[path = "native_dispatch_transport.rs"]
mod native_dispatch_transport;
fn hold(work: &str) -> crate::central_claims::NativeHeldWork {
    crate::central_claims::NativeHeldWork {
        run_id: uuid::Uuid::new_v4().to_string(),
        node_id: "worker".into(),
        context_sha256: "a".repeat(64),
        profile: "real".into(),
        work_id: work.into(),
        reserves_intake: true,
    }
}
#[test]
fn remote_ownership_reserves_intake_but_never_blocks_its_own_authorization() {
    let own = hold("#1");
    let other = hold("#2");
    let mut snapshot = super::super::tests::empty_snapshot();
    snapshot.profile.max_open_managed_mrs = 2;
    project_native_holds(
        "real",
        Some(&mut snapshot),
        &[own.clone(), other.clone()],
        None,
    )
    .unwrap();
    assert_eq!(snapshot.inflight_implementation_count, 2);
    assert!(snapshot.implementation_intake_paused);
    let mut authorizing = super::super::tests::empty_snapshot();
    authorizing.profile.max_open_managed_mrs = 2;
    project_native_holds(
        "real",
        Some(&mut authorizing),
        &[own.clone(), other],
        Some(&own.run_id),
    )
    .unwrap();
    assert_eq!(authorizing.inflight_implementation_count, 1);
    assert!(!authorizing.implementation_intake_paused);
}
#[test]
fn duplicate_or_other_profile_claim_projection_is_refused() {
    let first = hold("#1");
    let mut duplicate = hold("#1");
    assert!(project_native_holds("real", None, &[first.clone(), duplicate.clone()], None).is_err());
    duplicate.work_id = "#2".into();
    duplicate.profile = "other".into();
    assert!(project_native_holds("real", None, &[first, duplicate], None).is_err());
}
