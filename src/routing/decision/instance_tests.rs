use super::super::test_support::{defaults, implementation_request, path, profile};
use super::*;
use crate::config::{BackendInstanceConfig, CandidateConfig};
use tempfile::TempDir;

fn accounts() -> Profile {
    let mut profile = profile();
    for id in ["personal", "work"] {
        profile.routing.backend_instances.insert(
            id.into(),
            BackendInstanceConfig {
                runner_kind: "codex".into(),
                logical_backend: Some("codex".into()),
                executable: Some("/bin/sh".into()),
                account_label: Some(id.into()),
                quota_pool: Some(format!("{id}-quota")),
                supported_models: vec!["chosen-model".into()],
                ..Default::default()
            },
        );
    }
    profile.routing.improve_candidates = Some(vec![CandidateConfig {
        backend: "codex".into(),
        instance: Some("personal".into()),
        model: Some("chosen-model".into()),
        quota_pool: Some("personal-candidate-pool".into()),
        included_in_quota: true,
        ..Default::default()
    }]);
    profile
}

fn pinned() -> RouteRequest<'static> {
    RouteRequest {
        requested_backend: "codex",
        requested_instance: Some("work"),
        requested_model: Some("chosen-model"),
        ..implementation_request()
    }
}

fn select(profile: &Profile, request: RouteRequest<'_>) -> Result<RouteDecision> {
    let tmp = TempDir::new().unwrap();
    decide_with(
        &defaults(),
        profile,
        request,
        &path(&tmp),
        OffsetDateTime::now_utc(),
        |_| true,
    )
}

#[test]
fn explicit_account_uses_only_its_identity_and_never_borrows_sibling_quota_metadata() {
    let decision = select(&accounts(), pinned()).unwrap();
    assert_eq!(decision.identity.backend_instance, "work");
    assert_eq!(decision.identity.account_label.as_deref(), Some("work"));
    assert_eq!(decision.identity.quota_pool.as_deref(), Some("work-quota"));
    assert_eq!(
        decision.identity.effective_model.as_deref(),
        Some("chosen-model")
    );
    assert!(decision.identity.explicit_instance);
}

#[test]
fn unavailable_selected_account_does_not_run_a_healthy_sibling() {
    let mut profile = accounts();
    profile
        .routing
        .backend_instances
        .get_mut("work")
        .unwrap()
        .enabled = Some(false);
    let error = select(&profile, pinned()).unwrap_err();
    assert!(error.downcast_ref::<RouteError>().is_some());
    assert!(
        error.to_string().contains("backend instance disabled"),
        "{error:#}"
    );
}

#[test]
fn explicit_account_rejects_unknown_wrong_backend_and_unsupported_models() {
    for (request, reason) in [
        (
            RouteRequest {
                requested_instance: Some("missing"),
                ..pinned()
            },
            "not declared",
        ),
        (
            RouteRequest {
                requested_backend: "claude",
                ..pinned()
            },
            "does not run backend",
        ),
        (
            RouteRequest {
                requested_model: Some("other-model"),
                ..pinned()
            },
            "does not support",
        ),
        (
            RouteRequest {
                requested_backend: "auto",
                ..pinned()
            },
            "requires an explicit backend",
        ),
    ] {
        let error = select(&accounts(), request).unwrap_err();
        assert!(error.to_string().contains(reason), "{error:#}");
    }
}

#[test]
fn an_account_pin_cannot_bypass_an_instance_specific_model_allow_list() {
    let mut profile = accounts();
    profile.routing.allowed_models.insert(
        "improve".into(),
        vec![CandidateConfig {
            backend: "codex".into(),
            instance: Some("personal".into()),
            model: Some("chosen-model".into()),
            ..Default::default()
        }],
    );
    let error = select(&profile, pinned()).unwrap_err();
    assert!(
        error.to_string().contains("not in routing.allowed_models"),
        "{error:#}"
    );
    profile.routing.allowed_models.get_mut("improve").unwrap()[0].instance = Some("work".into());
    assert_eq!(
        select(&profile, pinned())
            .unwrap()
            .identity
            .backend_instance,
        "work"
    );
}

#[test]
fn configured_paid_approval_applies_to_the_selected_account() {
    let mut profile = accounts();
    profile
        .routing
        .improve_candidates
        .as_mut()
        .unwrap()
        .push(CandidateConfig {
            backend: "codex".into(),
            instance: Some("work".into()),
            model: Some("chosen-model".into()),
            requires_approval: true,
            ..Default::default()
        });
    let error = select(&profile, pinned()).unwrap_err();
    assert!(
        error.to_string().contains("operator_approval_required"),
        "{error:#}"
    );
}

#[test]
fn configured_default_model_is_resolved_from_the_selected_account() {
    let mut profile = accounts();
    profile
        .routing
        .backend_instances
        .get_mut("personal")
        .unwrap()
        .supported_models = vec!["personal-default".into()];
    let decision = select(
        &profile,
        RouteRequest {
            requested_model: None,
            ..pinned()
        },
    )
    .unwrap();
    assert_eq!(
        decision.identity.effective_model.as_deref(),
        Some("chosen-model")
    );
}
