use super::*;
use std::os::unix::fs::PermissionsExt;

fn fixture() -> (tempfile::TempDir, crate::config::GahConfig) {
    let tmp = tempfile::tempdir().unwrap();
    let mut profile = crate::config::tests::test_profile_for_notifications();
    profile.policy_path = Some(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/policy-read-only.toml"
        )
        .into(),
    );
    profile.artifact_root = tmp.path().display().to_string();
    let cfg = crate::config::GahConfig {
        context: Default::default(),
        defaults: crate::config::Defaults {
            artifact_root: profile.artifact_root.clone(),
            ..Default::default()
        },
        profiles: std::collections::HashMap::from([("test".into(), profile)]),
    };
    (tmp, cfg)
}

struct ProviderPath;
impl Drop for ProviderPath {
    fn drop(&mut self) {
        crate::provider::clear_test_provider_path();
    }
}

fn refuse_without_provider(
    cfg: &crate::config::GahConfig,
    tmp: &std::path::Path,
    action: &NextAction,
) {
    let _exec = crate::test_support::ExecGuard::new();
    let calls = tmp.join("calls");
    for bin in ["gh", "glab"] {
        let path = tmp.join(bin);
        std::fs::write(
            &path,
            format!("#!/bin/sh\necho called >> '{}'\nexit 1\n", calls.display()),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    crate::provider::set_test_provider_path(tmp.to_str().unwrap());
    let _path = ProviderPath;
    let outcome = execute_action(cfg, "test", action, &[], true, None).unwrap();
    assert!(outcome.starts_with("Blocked "), "{outcome}");
    assert!(outcome.contains("POLICY BLOCKED"), "{outcome}");
    assert!(
        !calls.exists(),
        "a policy refusal must not call the provider"
    );
    let entries = crate::ledger::read_entries(cfg).unwrap();
    let profile = &cfg.profiles["test"];
    let gate = crate::ledger::effective_human_gate_from_entries(
        &entries,
        "test",
        &profile.repo_id,
        "#1544",
    )
    .unwrap();
    assert_eq!(gate.reason_code.as_deref(), Some("policy_approval"));
    assert_eq!(gate.dispatch_reason.as_deref(), Some("repo_policy_blocked"));
    let events = crate::events::read_events(cfg).unwrap();
    let finished = events
        .iter()
        .find(|event| event.event_type == "dispatch_finished")
        .unwrap();
    assert_eq!(finished.reason_code.as_deref(), Some("policy_approval"));
}

#[test]
fn merge_refused_when_policy_read_only() {
    for (provider, policy) in [
        ("github", crate::config::MergePolicy::Auto),
        ("gitlab", crate::config::MergePolicy::Auto),
        ("gitlab", crate::config::MergePolicy::GitlabMwps),
    ] {
        let (tmp, mut cfg) = fixture();
        let profile = cfg.profiles.get_mut("test").unwrap();
        profile.provider = provider.into();
        profile.routing.merge_policy = Some(policy);
        let action = NextAction::MergeMr {
            branch: "gah/1544".into(),
            work_id: Some("#1544".into()),
            mr_url: None,
            review_generation: Some("generation".into()),
            reason: "approved".into(),
        };
        refuse_without_provider(&cfg, tmp.path(), &action);
    }
}

#[test]
fn mark_ready_refused_when_policy_read_only() {
    let (tmp, cfg) = fixture();
    let action = NextAction::MarkReadyForReview {
        branch: "gah/1544".into(),
        work_id: Some("#1544".into()),
        mr_url: None,
        reason: "review passed".into(),
    };
    refuse_without_provider(&cfg, tmp.path(), &action);
}
