use super::*;

#[test]
fn same_runner_and_model_instances_remain_independently_routable() {
    let tmp = TempDir::new().unwrap();
    let mut profile = profile();
    for name in ["opencode-subscription", "opencode-api"] {
        profile.routing.backend_instances.insert(
            name.into(),
            crate::config::BackendInstanceConfig {
                runner_kind: "opencode".into(),
                logical_backend: Some("opencode".into()),
                executable: Some("/bin/sh".into()),
                ..Default::default()
            },
        );
    }
    let mut subscription = candidate_config("opencode", Some("openai/gpt-5"), None);
    subscription.instance = Some("opencode-subscription".into());
    subscription.priority = 100;
    subscription.included_in_quota = true;
    let mut api = candidate_config("opencode", Some("openai/gpt-5"), None);
    api.instance = Some("opencode-api".into());
    api.priority = 100;
    api.marginal_cost_usd = Some(1.0);
    profile.routing.improve_candidates = Some(vec![subscription, api]);
    let mut runtime = RoutingRuntimeState::default();
    runtime.recent_runs.insert(
        CandidateIdentity::new("opencode-subscription", Some("openai/gpt-5")),
        4,
    );
    runtime.recent_runs.insert(
        CandidateIdentity::new("opencode-api", Some("openai/gpt-5")),
        0,
    );

    let decision = decide_with_runtime(
        &defaults(),
        &profile,
        RouteRequest {
            mode: "improve",
            requested_backend: "auto",
            requested_instance: None,
            requested_model: None,
            recommended_backend: None,
            recommended_model: None,
            session_id: None,
            usage_summary: None,
            last_failure_class: None,
            exact_route_required: false,
        },
        &runtime,
        &path(&tmp),
        OffsetDateTime::now_utc(),
        backend_available,
    )
    .unwrap();

    assert_eq!(decision.identity.backend_instance, "opencode-api");
    assert_eq!(decision.effective_backend, "opencode");
    assert_eq!(decision.effective_model.as_deref(), Some("openai/gpt-5"));
    assert_eq!(
        decision
            .routing_diagnostics
            .as_ref()
            .map(|diagnostics| diagnostics.candidates.len()),
        Some(2)
    );
}

#[test]
fn explicit_instance_without_executable_cannot_fall_back_to_path() {
    let tmp = TempDir::new().unwrap();
    let mut profile = profile();
    for (name, executable) in [("broken", None), ("working", Some("/bin/sh"))] {
        profile.routing.backend_instances.insert(
            name.into(),
            crate::config::BackendInstanceConfig {
                runner_kind: "opencode".into(),
                logical_backend: Some("opencode".into()),
                executable: executable.map(str::to_string),
                ..Default::default()
            },
        );
    }
    let mut broken = candidate_config("opencode", Some("openai/gpt-5"), None);
    broken.instance = Some("broken".into());
    let mut working = broken.clone();
    working.instance = Some("working".into());
    profile.routing.improve_candidates = Some(vec![broken, working]);

    let decision = decide_with(
        &defaults(),
        &profile,
        RouteRequest {
            mode: "improve",
            requested_backend: "auto",
            requested_instance: None,
            requested_model: None,
            recommended_backend: None,
            recommended_model: None,
            session_id: None,
            usage_summary: None,
            last_failure_class: None,
            exact_route_required: false,
        },
        &path(&tmp),
        OffsetDateTime::now_utc(),
        |_| true,
    )
    .unwrap();

    assert_eq!(decision.identity.backend_instance, "working");
    assert!(decision
        .routing_reason
        .contains("backend CLI not installed"));
}

#[test]
fn paid_instance_approval_error_names_exact_destination() {
    let tmp = TempDir::new().unwrap();
    let mut profile = profile();
    profile.routing.backend_instances.insert(
        "opencode-api".into(),
        crate::config::BackendInstanceConfig {
            runner_kind: "opencode".into(),
            logical_backend: Some("opencode".into()),
            executable: Some("/bin/sh".into()),
            ..Default::default()
        },
    );
    let mut paid = candidate_config("opencode", Some("openai/gpt-5"), None);
    paid.instance = Some("opencode-api".into());
    paid.requires_approval = true;
    profile.routing.improve_candidates = Some(vec![paid]);

    let error = decide_with(
        &defaults(),
        &profile,
        RouteRequest {
            mode: "improve",
            requested_backend: "auto",
            requested_instance: None,
            requested_model: None,
            recommended_backend: None,
            recommended_model: None,
            session_id: None,
            usage_summary: None,
            last_failure_class: None,
            exact_route_required: false,
        },
        &path(&tmp),
        OffsetDateTime::now_utc(),
        backend_available,
    )
    .unwrap_err();
    let route_error = error.downcast_ref::<RouteError>().unwrap();

    assert!(matches!(
        route_error,
        RouteError::ApprovalRequired {
            backend_instance: Some(instance),
            ..
        } if instance == "opencode-api"
    ));
    assert!(route_error.to_string().contains("instance opencode-api"));
}
