use super::{
    add_profile, canonical_backend_name, get_profile_mut, load, load_canonical_routing,
    merge_routing_policy, remove_profile, save, CandidateConfig, GahConfig, Profile, RoutingPolicy,
};
use crate::test_support::{clear_canonical_config_override, set_canonical_config_override};

/// Build a complete `Profile` for tests in other modules.
pub fn test_profile_for_notifications() -> Profile {
    Profile {
        manager_wake_autonomy: crate::config::WakeAutonomy::default(),
        delivery_mode: crate::config::DeliveryMode::default(),
        prune_older_than_days: None,
        chat_session_idle_days: None,
        display_name: "Repo".into(),
        repo_id: "repo".into(),
        provider: "github".into(),
        repo: "owner/repo".into(),
        local_path: "/tmp/repo".into(),
        artifact_root: "/tmp/artifacts".into(),
        default_target_branch: "main".into(),
        provider_api_base: None,
        provider_project_id: None,
        oh_profile: None,
        skill_resolve_failure_policy: Default::default(),
        openhands_args: vec![],
        codex_args: vec![],
        codex_path: None,
        claude_args: vec![],
        claude_path: None,
        agy_path: None,
        vibe_args: vec![],
        vibe_path: None,
        opencode_args: vec![],
        opencode_path: None,
        hermes_args: vec![],
        hermes_path: None,
        agy_second_home: None,
        agy_print_timeout_seconds: std::collections::HashMap::new(),
        agy_idle_timeout_seconds: None,
        opencode_idle_timeout_seconds: None,
        opencode_idle_timeout_seconds_by_model: std::collections::HashMap::new(),
        max_concurrent_per_model: std::collections::HashMap::new(),
        openhands_idle_timeout_seconds: None,
        vibe_idle_timeout_seconds: None,
        codex_idle_timeout_seconds: None,
        claude_idle_timeout_seconds: None,
        hermes_idle_timeout_seconds: None,
        max_parallel_workers: None,
        max_open_managed_mrs: None,
        worker_scaling: Default::default(),
        notify_command: None,
        policy_path: None,
        env_file: None,
        env_file_prod: None,
        validation_commands: vec![],
        auto_fix_commands: vec![],
        test_file_patterns: vec![],
        known_baseline_failure_markers: vec![],
        model_improve: None,
        model_pm: None,
        model_review: None,
        review_timeout_seconds: None,
        review_hard_timeout_seconds: None,
        validation_timeout_seconds: None,
        routing: RoutingPolicy::default(),
        publishing: Default::default(),
        external_credential_scopes: std::collections::HashMap::new(),
        pacing: Default::default(),
    }
}

fn gitlab_profile(api_base: Option<&str>) -> Profile {
    Profile {
        display_name: "Test".into(),
        repo_id: "test".into(),
        provider: "gitlab".into(),
        repo: "group/repo".into(),
        provider_api_base: api_base.map(str::to_string),
        provider_project_id: Some("42".into()),
        ..test_profile_for_notifications()
    }
}

fn config_with_one_profile() -> GahConfig {
    let mut profiles = std::collections::HashMap::new();
    profiles.insert("test".to_string(), test_profile_for_notifications());
    GahConfig {
        context: Default::default(),
        defaults: Default::default(),
        profiles,
    }
}

#[test]
fn add_profile_inserts_new_entry() {
    let mut cfg = config_with_one_profile();
    add_profile(&mut cfg, "second", gitlab_profile(None)).unwrap();
    assert!(cfg.profiles.contains_key("second"));
    assert_eq!(cfg.profiles.len(), 2);
}

#[test]
fn add_profile_rejects_duplicate_name() {
    let mut cfg = config_with_one_profile();
    let err = add_profile(&mut cfg, "test", gitlab_profile(None)).unwrap_err();
    assert!(err.to_string().contains("already exists"));
    // Original profile must be untouched by the rejected add.
    assert_eq!(cfg.profiles.get("test").unwrap().provider, "github");
}

#[test]
fn remove_profile_deletes_existing_entry() {
    let mut cfg = config_with_one_profile();
    remove_profile(&mut cfg, "test").unwrap();
    assert!(!cfg.profiles.contains_key("test"));
}

#[test]
fn remove_profile_errors_on_missing_name() {
    let mut cfg = config_with_one_profile();
    let err = remove_profile(&mut cfg, "nonexistent").unwrap_err();
    assert!(err.to_string().contains("not found"));
    // Nothing should have been removed.
    assert_eq!(cfg.profiles.len(), 1);
}

#[test]
fn get_profile_mut_allows_in_place_field_update() {
    let mut cfg = config_with_one_profile();
    {
        let profile = get_profile_mut(&mut cfg, "test").unwrap();
        profile.display_name = "Renamed".to_string();
    }
    assert_eq!(cfg.profiles.get("test").unwrap().display_name, "Renamed");
}

#[test]
fn get_profile_mut_errors_on_missing_name_and_lists_available() {
    let mut cfg = config_with_one_profile();
    let err = get_profile_mut(&mut cfg, "nonexistent").unwrap_err();
    assert!(err.to_string().contains("not found"));
    assert!(err.to_string().contains("test"));
}

#[test]
fn save_round_trips_a_profile_through_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    let cfg = config_with_one_profile();
    save(&cfg, Some(path.to_str().unwrap())).unwrap();

    let reloaded = load(Some(path.to_str().unwrap())).unwrap();
    let profile = reloaded.profiles.get("test").unwrap();
    assert_eq!(profile.display_name, "Repo");
    assert_eq!(profile.repo, "owner/repo");
}

// Regression: configured_backend_path("openhands") used to return
// oh_profile (e.g. "nous-hy3"), which resolve_backend_executable in
// `runner::resolve` then treated as a literal executable file path --
// that string is never a real file, so every explicit `--backend
// openhands` dispatch on a profile with oh_profile set silently routed
// away from openhands as if it were unavailable. configured_backend_path
// must never return anything for openhands; is_backend_configured is the
// right function for "is this set up" instead.
#[test]
fn configured_backend_path_never_returns_a_value_for_openhands() {
    let mut profile = test_profile_for_notifications();
    profile.oh_profile = Some("nous-hy3".to_string());
    assert_eq!(profile.configured_backend_path("openhands"), None);
}

#[test]
fn is_backend_configured_true_for_openhands_with_oh_profile_set() {
    let mut profile = test_profile_for_notifications();
    profile.oh_profile = Some("nous-hy3".to_string());
    assert!(profile.is_backend_configured("openhands"));
}

#[test]
fn is_backend_configured_false_for_openhands_without_oh_profile() {
    let profile = test_profile_for_notifications();
    assert_eq!(profile.oh_profile, None);
    assert!(!profile.is_backend_configured("openhands"));
}

#[test]
fn is_backend_configured_delegates_to_path_for_other_backends() {
    let mut profile = test_profile_for_notifications();
    assert!(!profile.is_backend_configured("codex"));
    profile.codex_path = Some("/usr/local/bin/codex".to_string());
    assert!(profile.is_backend_configured("codex"));
}

#[test]
fn gitlab_push_url_uses_self_hosted_domain() {
    let profile = gitlab_profile(Some("https://gitlab.coltonspurgin.tech/api/v4"));
    assert_eq!(
        profile.push_url().unwrap(),
        "https://oauth2@gitlab.coltonspurgin.tech/group/repo.git"
    );
}

#[test]
fn gitlab_push_url_handles_trailing_slash_and_missing_api_suffix() {
    let profile = gitlab_profile(Some("https://gitlab.example.com/"));
    assert_eq!(
        profile.push_url().unwrap(),
        "https://oauth2@gitlab.example.com/group/repo.git"
    );
}

#[test]
fn gitlab_push_url_rejects_missing_host() {
    let profile = gitlab_profile(Some("https:///api/v4"));
    assert!(profile.push_url().is_err());
}

#[test]
fn web_url_github_is_a_clickable_repo_link() {
    let profile = test_profile_for_notifications();
    assert_eq!(profile.web_url().unwrap(), "https://github.com/owner/repo");
}

#[test]
fn web_url_gitlab_uses_self_hosted_domain_without_credentials_or_git_suffix() {
    let profile = gitlab_profile(Some("https://gitlab.coltonspurgin.tech/api/v4"));
    assert_eq!(
        profile.web_url().unwrap(),
        "https://gitlab.coltonspurgin.tech/group/repo"
    );
}

#[test]
fn web_url_none_when_provider_unrecognized_or_gitlab_misconfigured() {
    let mut profile = gitlab_profile(None);
    assert!(profile.web_url().is_none());
    profile.provider = "bitbucket".into();
    assert!(profile.web_url().is_none());
}

#[test]
fn merge_keeps_repo_value_when_both_set() {
    let canonical = RoutingPolicy {
        default_backend: Some("codex".into()),
        ..Default::default()
    };
    let repo = RoutingPolicy {
        default_backend: Some("claude".into()),
        ..Default::default()
    };
    let merged = merge_routing_policy(canonical, repo);
    assert_eq!(merged.default_backend.as_deref(), Some("claude"));
}

#[test]
fn merge_fills_gap_from_canonical_when_repo_unset() {
    let canonical = RoutingPolicy {
        default_backend: Some("codex".into()),
        review_backend: Some("claude".into()),
        ..Default::default()
    };
    let repo = RoutingPolicy::default();
    let merged = merge_routing_policy(canonical, repo);
    assert_eq!(merged.default_backend.as_deref(), Some("codex"));
    assert_eq!(merged.review_backend.as_deref(), Some("claude"));
}

#[test]
fn merge_nested_override_does_not_erase_unrelated_inherited_fields() {
    // TICKET-106 AC: "one nested override does not erase unrelated
    // inherited values" -- repo overrides only default_backend, and
    // review_backend must still come from canonical.
    let canonical = RoutingPolicy {
        default_backend: Some("codex".into()),
        review_backend: Some("claude".into()),
        ..Default::default()
    };
    let repo = RoutingPolicy {
        default_backend: Some("agy".into()),
        ..Default::default()
    };
    let merged = merge_routing_policy(canonical, repo);
    assert_eq!(merged.default_backend.as_deref(), Some("agy"));
    assert_eq!(merged.review_backend.as_deref(), Some("claude"));
}

#[test]
fn merge_candidate_list_replaces_wholesale_not_concatenates() {
    let canonical = RoutingPolicy {
        improve_candidates: Some(vec![CandidateConfig {
            backend: "codex".into(),
            instance: None,
            model: None,
            quota_pool: None,
            priority: 0,
            included_in_quota: false,
            marginal_cost_usd: None,
            requires_approval: false,
            ..Default::default()
        }]),
        ..Default::default()
    };
    let repo = RoutingPolicy {
        improve_candidates: Some(vec![CandidateConfig {
            backend: "claude".into(),
            instance: None,
            model: None,
            quota_pool: None,
            priority: 0,
            included_in_quota: false,
            marginal_cost_usd: None,
            requires_approval: false,
            ..Default::default()
        }]),
        ..Default::default()
    };
    let merged = merge_routing_policy(canonical, repo);
    let candidates = merged.improve_candidates.unwrap();
    assert_eq!(candidates.len(), 1, "replace, not concatenate");
    assert_eq!(candidates[0].backend, "claude");
}

#[test]
fn merge_capability_map_merges_by_key() {
    use std::collections::HashMap;
    let mut canonical_caps = HashMap::new();
    canonical_caps.insert("claude".to_string(), vec!["ponytail".to_string()]);
    let canonical = RoutingPolicy {
        review_required_capabilities: canonical_caps,
        ..Default::default()
    };
    let mut repo_caps = HashMap::new();
    repo_caps.insert("codex".to_string(), vec!["something-else".to_string()]);
    let repo = RoutingPolicy {
        review_required_capabilities: repo_caps,
        ..Default::default()
    };
    let merged = merge_routing_policy(canonical, repo);
    assert_eq!(
        merged.review_required_capabilities.get("claude"),
        Some(&vec!["ponytail".to_string()])
    );
    assert_eq!(
        merged.review_required_capabilities.get("codex"),
        Some(&vec!["something-else".to_string()])
    );
}

#[test]
fn merge_capability_map_repo_key_overrides_canonical_same_key() {
    use std::collections::HashMap;
    let mut canonical_caps = HashMap::new();
    canonical_caps.insert("claude".to_string(), vec!["ponytail".to_string()]);
    let canonical = RoutingPolicy {
        review_required_capabilities: canonical_caps,
        ..Default::default()
    };
    let mut repo_caps = HashMap::new();
    repo_caps.insert("claude".to_string(), vec![]);
    let repo = RoutingPolicy {
        review_required_capabilities: repo_caps,
        ..Default::default()
    };
    let merged = merge_routing_policy(canonical, repo);
    assert_eq!(
        merged.review_required_capabilities.get("claude"),
        Some(&vec![])
    );
}

#[test]
fn merge_with_all_default_canonical_is_a_no_op() {
    // TICKET-106 AC: "missing shared defaults preserves backward-
    // compatible behavior" -- an all-default canonical (equivalent to
    // no canonical file existing) must not change repo's routing at all.
    let repo = RoutingPolicy {
        default_backend: Some("codex".into()),
        allow_review_fallback: true,
        ..Default::default()
    };
    let merged = merge_routing_policy(RoutingPolicy::default(), repo.clone());
    assert_eq!(merged.default_backend, repo.default_backend);
    assert_eq!(merged.allow_review_fallback, repo.allow_review_fallback);
}

#[test]
fn load_canonical_routing_is_none_when_file_does_not_exist() {
    let tmp = tempfile::tempdir().unwrap();
    set_canonical_config_override(tmp.path().join("does-not-exist.toml"));
    let result = load_canonical_routing().unwrap();
    clear_canonical_config_override();
    assert!(result.is_none());
}

#[test]
fn load_canonical_routing_fails_loudly_on_malformed_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("canonical.toml");
    std::fs::write(&path, "not valid toml [[[").unwrap();
    set_canonical_config_override(&path);
    let result = load_canonical_routing();
    clear_canonical_config_override();
    assert!(result.is_err());
}

#[test]
fn load_merges_canonical_into_repo_defaults_routing() {
    let tmp = tempfile::tempdir().unwrap();
    let canonical_path = tmp.path().join("canonical.toml");
    std::fs::write(
        &canonical_path,
        "[routing]\ndefault_backend = \"codex\"\nreview_backend = \"claude\"\n",
    )
    .unwrap();
    set_canonical_config_override(&canonical_path);

    let repo_config_path = tmp.path().join("gah-config.toml");
    std::fs::write(
        &repo_config_path,
        "[defaults]\nartifact_root = \"\"\nworktree_base = \"\"\nllm_base_url = \"\"\nllm_model_local = \"\"\nllm_model_cloud = \"\"\n[defaults.routing]\ndefault_backend = \"agy\"\n",
    )
    .unwrap();

    let cfg = load(Some(repo_config_path.to_str().unwrap())).unwrap();
    clear_canonical_config_override();

    // repo's own default_backend wins...
    assert_eq!(cfg.defaults.routing.default_backend.as_deref(), Some("agy"));
    // ...but review_backend, which repo never set, inherits from canonical.
    assert_eq!(
        cfg.defaults.routing.review_backend.as_deref(),
        Some("claude")
    );
}

#[test]
fn two_different_repo_configs_both_inherit_the_same_canonical_routing() {
    // TICKET-106 AC: "World Cup can inherit canonical routing while
    // overriding repo-specific behavior" -- simulated with two throwaway
    // repo configs rather than touching a real second repo.
    let tmp = tempfile::tempdir().unwrap();
    let canonical_path = tmp.path().join("canonical.toml");
    std::fs::write(
        &canonical_path,
        "[routing]\nreview_backend = \"claude\"\nstrong_review_backend = \"claude\"\n",
    )
    .unwrap();
    set_canonical_config_override(&canonical_path);

    let minimal_repo_path = tmp.path().join("minimal-repo.toml");
    std::fs::write(
        &minimal_repo_path,
        "[defaults]\nartifact_root = \"\"\nworktree_base = \"\"\nllm_base_url = \"\"\nllm_model_local = \"\"\nllm_model_cloud = \"\"\n",
    )
    .unwrap();

    let overriding_repo_path = tmp.path().join("overriding-repo.toml");
    std::fs::write(
        &overriding_repo_path,
        "[defaults]\nartifact_root = \"\"\nworktree_base = \"\"\nllm_base_url = \"\"\nllm_model_local = \"\"\nllm_model_cloud = \"\"\n[defaults.routing]\nreview_backend = \"codex\"\n",
    )
    .unwrap();

    let minimal_cfg = load(Some(minimal_repo_path.to_str().unwrap())).unwrap();
    let overriding_cfg = load(Some(overriding_repo_path.to_str().unwrap())).unwrap();
    clear_canonical_config_override();

    // A minimal repo config with no routing section at all still
    // receives canonical routing automatically.
    assert_eq!(
        minimal_cfg.defaults.routing.review_backend.as_deref(),
        Some("claude")
    );
    assert_eq!(
        minimal_cfg
            .defaults
            .routing
            .strong_review_backend
            .as_deref(),
        Some("claude")
    );
    // A repo that overrides one field keeps its own value for that
    // field, but still inherits the other canonical field unchanged.
    assert_eq!(
        overriding_cfg.defaults.routing.review_backend.as_deref(),
        Some("codex")
    );
    assert_eq!(
        overriding_cfg
            .defaults
            .routing
            .strong_review_backend
            .as_deref(),
        Some("claude")
    );
}

#[test]
fn shipped_canonical_example_parses_as_a_canonical_config() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/config/gah-canonical.example.toml"
    ))
    .unwrap();
    let canonical: super::CanonicalConfig = toml::from_str(&text).unwrap();
    // Issue #123: the shipped canonical example uses the new two-tier
    // reviewer scheme (routine_reviewer + escalatory_reviewers list).
    let routine = canonical.routing.effective_routine_reviewer().unwrap();
    assert_eq!(routine.backend, "claude");
    assert_eq!(routine.model.as_deref(), Some("sonnet"));
    let escalatory = canonical.routing.effective_escalatory_reviewers();
    assert_eq!(escalatory.len(), 1);
    assert_eq!(escalatory[0].backend, "codex");
    assert_eq!(
        canonical.routing.review_required_capabilities.get("claude"),
        Some(&vec!["ponytail".to_string()])
    );
}
#[test]
fn routing_policy_inherits_missing_fields_from_defaults() {
    let defaults = RoutingPolicy {
        default_backend: Some("codex".into()),
        pm_backend: Some("claude".into()),
        pm_candidates: Some(vec![super::CandidateConfig {
            backend: "claude".into(),
            instance: None,
            model: Some("sonnet".into()),
            quota_pool: Some("claude-main".into()),
            priority: 2,
            included_in_quota: true,
            marginal_cost_usd: Some(0.0),
            requires_approval: false,
            ..Default::default()
        }]),
        task_routing_rules: vec![super::TaskRoutingRule {
            modes: vec!["improve".into()],
            task_classes: vec!["documentation".into()],
            difficulties: vec!["easy".into()],
            risks: vec!["low".into()],
            candidates: vec![super::CandidateConfig {
                backend: "agy".into(),
                model: Some("cheap".into()),
                ..super::CandidateConfig::default()
            }],
        }],
        allow_review_fallback: true,
        max_runs_per_backend_per_week: Some(3),
        ..RoutingPolicy::default()
    };
    let profile = RoutingPolicy {
        improve_backend: Some("agy".into()),
        ..RoutingPolicy::default()
    };

    let merged = profile.merged_with_defaults(&defaults);

    assert_eq!(merged.default_backend.as_deref(), Some("codex"));
    assert_eq!(merged.pm_backend.as_deref(), Some("claude"));
    assert_eq!(merged.improve_backend.as_deref(), Some("agy"));
    assert_eq!(merged.pm_candidates.as_ref().map(Vec::len), Some(1));
    assert!(merged.allow_review_fallback);
    assert_eq!(merged.max_runs_per_backend_per_week, Some(3));
    assert_eq!(merged.task_routing_rules.len(), 1);
    assert_eq!(
        merged.task_routing_rules[0].candidates[0].model.as_deref(),
        Some("cheap")
    );
}

#[test]
fn routing_policy_profile_candidate_list_replaces_default_list() {
    let defaults = RoutingPolicy {
        pm_candidates: Some(vec![super::CandidateConfig {
            backend: "claude".into(),
            instance: None,
            model: Some("sonnet".into()),
            quota_pool: None,
            priority: 0,
            included_in_quota: false,
            marginal_cost_usd: None,
            requires_approval: false,
            ..Default::default()
        }]),
        ..RoutingPolicy::default()
    };
    let profile = RoutingPolicy {
        pm_candidates: Some(vec![super::CandidateConfig {
            backend: "codex".into(),
            instance: None,
            model: Some("gpt-5".into()),
            quota_pool: None,
            priority: 0,
            included_in_quota: false,
            marginal_cost_usd: None,
            requires_approval: false,
            ..Default::default()
        }]),
        ..RoutingPolicy::default()
    };

    let merged = profile.merged_with_defaults(&defaults);

    let candidates = merged.pm_candidates.expect("merged candidate list");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].backend, "codex");
    assert_eq!(candidates[0].model.as_deref(), Some("gpt-5"));
}

#[test]
fn task_routing_rules_are_profile_overridable_as_one_ordered_policy() {
    let rule = |backend: &str| super::TaskRoutingRule {
        modes: vec!["improve".into()],
        task_classes: vec!["documentation".into()],
        difficulties: vec!["easy".into()],
        risks: vec!["low".into()],
        candidates: vec![super::CandidateConfig {
            backend: backend.into(),
            ..super::CandidateConfig::default()
        }],
    };
    let defaults = RoutingPolicy {
        task_routing_rules: vec![rule("agy")],
        ..RoutingPolicy::default()
    };
    let profile = RoutingPolicy {
        task_routing_rules: vec![rule("codex")],
        ..RoutingPolicy::default()
    };

    let merged = profile.merged_with_defaults(&defaults);
    assert_eq!(merged.task_routing_rules.len(), 1);
    assert_eq!(merged.task_routing_rules[0].candidates[0].backend, "codex");
}

#[test]
fn review_budget_defaults_and_profile_override_are_deterministic() {
    let defaults = RoutingPolicy::default();
    assert_eq!(defaults.max_review_cycles_per_ticket(), 3);
    assert_eq!(defaults.max_fix_attempts_per_mr(), 2);
    assert_eq!(defaults.max_paid_reviews_per_ticket(), 3);
    assert_eq!(defaults.max_implementation_failures_per_ticket(), 8);

    let global = RoutingPolicy {
        max_review_cycles_per_ticket: Some(4),
        max_fix_attempts_per_mr: Some(3),
        max_paid_reviews_per_ticket: Some(5),
        max_implementation_failures_per_ticket: Some(10),
        ..RoutingPolicy::default()
    };
    let profile = RoutingPolicy {
        max_review_cycles_per_ticket: Some(1),
        max_fix_attempts_per_mr: Some(4),
        ..RoutingPolicy::default()
    };
    let effective = profile.merged_with_defaults(&global);
    assert_eq!(effective.max_review_cycles_per_ticket(), 1);
    assert_eq!(effective.max_fix_attempts_per_mr(), 4);
    assert_eq!(effective.max_paid_reviews_per_ticket(), 5);
    assert_eq!(effective.max_implementation_failures_per_ticket(), 10);

    let repair_budget_only = RoutingPolicy {
        max_fix_attempts_per_mr: Some(4),
        ..RoutingPolicy::default()
    };
    assert_eq!(repair_budget_only.max_review_cycles_per_ticket(), 5);
}

#[test]
fn profile_effective_routing_preserves_legacy_standalone_behavior() {
    let config: GahConfig = toml::from_str(
        r#"
[profiles.repo]
display_name = "Repo"
repo_id = "repo"
provider = "github"
repo = "owner/repo"
local_path = "/tmp/repo"
artifact_root = "/tmp/artifacts"
default_target_branch = "main"

[profiles.repo.routing]
pm_backend = "claude"
"#,
    )
    .unwrap();

    let profile = config.profiles.get("repo").unwrap();
    let effective = profile.effective_routing(&config.defaults);

    assert_eq!(effective.pm_backend.as_deref(), Some("claude"));
    assert_eq!(effective.default_backend, None);
}

#[test]
fn profile_effective_routing_inherits_defaults_field_by_field() {
    let config: GahConfig = toml::from_str(
        r#"
[defaults.routing]
default_backend = "codex"
pm_candidates = [{ backend = "claude", model = "sonnet" }]
allow_review_fallback = true

[profiles.repo]
display_name = "Repo"
repo_id = "repo"
provider = "github"
repo = "owner/repo"
local_path = "/tmp/repo"
artifact_root = "/tmp/artifacts"
default_target_branch = "main"

[profiles.repo.routing]
improve_backend = "agy"
"#,
    )
    .unwrap();

    let profile = config.profiles.get("repo").unwrap();
    let effective = profile.effective_routing(&config.defaults);

    assert_eq!(effective.default_backend.as_deref(), Some("codex"));
    assert_eq!(effective.improve_backend.as_deref(), Some("agy"));
    assert_eq!(effective.pm_candidates.as_ref().map(Vec::len), Some(1));
    assert!(effective.allow_review_fallback);
}

// The cloud-coder → openhands merge is asserted against the same
// public function in tests/execution_identity.rs; only the
// pass-through cases are unit-tested here.
#[test]
fn canonical_backend_name_leaves_other_backends_and_auto_untouched() {
    // "auto" must NOT be rewritten here: its effective backend is
    // resolved dynamically per-attempt by routing::decide, not a fixed
    // alias.
    for name in ["auto", "codex", "claude", "vibe", "opencode", "agy"] {
        assert_eq!(canonical_backend_name(name), name);
    }
}
