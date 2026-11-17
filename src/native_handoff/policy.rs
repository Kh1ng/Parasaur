//! Portable job policy is an explicit allow-list. Runtime paths, credentials,
//! CLI arguments and notifications remain local to the executing node.
use crate::config::{GahConfig, Profile, RoutingPolicy};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const FIELDS: &[&str] = &[
    "display_name",
    "repo_id",
    "provider",
    "repo",
    "default_target_branch",
    "provider_api_base",
    "provider_project_id",
    "skill_resolve_failure_policy",
    "opencode_idle_timeout_seconds",
    "opencode_idle_timeout_seconds_by_model",
    "max_concurrent_per_model",
    "openhands_idle_timeout_seconds",
    "vibe_idle_timeout_seconds",
    "codex_idle_timeout_seconds",
    "claude_idle_timeout_seconds",
    "hermes_idle_timeout_seconds",
    "max_parallel_workers",
    "max_open_managed_mrs",
    "worker_scaling",
    "agy_print_timeout_seconds",
    "agy_idle_timeout_seconds",
    "delivery_mode",
    "validation_commands",
    "auto_fix_commands",
    "test_file_patterns",
    "known_baseline_failure_markers",
    "model_improve",
    "model_pm",
    "model_review",
    "review_timeout_seconds",
    "review_hard_timeout_seconds",
    "validation_timeout_seconds",
    "routing",
    "publishing",
    "pacing",
    "external_credential_scopes",
];

const LOCAL_FIELDS: &[&str] = &[
    "local_path",
    "artifact_root",
    "oh_profile",
    "openhands_args",
    "codex_args",
    "codex_path",
    "claude_args",
    "claude_path",
    "agy_path",
    "vibe_args",
    "vibe_path",
    "opencode_args",
    "opencode_path",
    "hermes_args",
    "hermes_path",
    "agy_second_home",
    "notify_command",
    "manager_wake_autonomy",
    "policy_path",
    "env_file",
    "env_file_prod",
    "prune_older_than_days",
    "chat_session_idle_days",
];

fn assert_profile_fields(value: &Value) -> Result<()> {
    ensure!(
        value
            .as_object()
            .context("Profile is not a policy object")?
            .keys()
            .all(|key| FIELDS.contains(&key.as_str()) || LOCAL_FIELDS.contains(&key.as_str())),
        "Profile schema changed; classify new fields before native policy transfer"
    );
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PortablePolicy {
    pub fields: Map<String, Value>,
    pub runtime_rules: BTreeMap<String, String>,
    pub context: crate::context::ContextConfig,
    pub llm_model_local: String,
    pub llm_model_cloud: String,
}

// These vectors can include permission flags as well as runtime options. Do
// not silently discard a central restriction or merge opposing CLI switches.
// Pin their node-local values by digest; paths and credentials stay local.
// The only exception is OpenCode's ignored agent override: both launch paths
// select that from the job role. Model pins still affect routing and stay pinned.
fn runtime_rules(value: &Value) -> Result<BTreeMap<String, String>> {
    [
        "openhands_args",
        "codex_args",
        "claude_args",
        "vibe_args",
        "opencode_args",
        "hermes_args",
        "oh_profile",
    ]
    .into_iter()
    .map(|key| {
        let raw = value
            .get(key)
            .context("Runtime rule field is unavailable")?;
        let canonical = if key == "opencode_args" {
            let args: Vec<String> = serde_json::from_value(raw.clone())
                .context("OpenCode runtime arguments are not a string vector")?;
            serde_json::to_value(crate::runner::resolve::without_opencode_agent_override(
                &args,
            ))?
        } else {
            raw.clone()
        };
        Ok((
            key.into(),
            format!("{:x}", Sha256::digest(serde_json::to_vec(&canonical)?)),
        ))
    })
    .collect()
}

pub(super) fn prepare(cfg: &GahConfig, profile_name: &str) -> Result<PortablePolicy> {
    let profile = crate::config::get_profile(cfg, profile_name)?;
    let mut source = serde_json::to_value(profile)?;
    assert_profile_fields(&source)?;
    let mut routing = profile.effective_routing(&cfg.defaults);
    for instance in routing.backend_instances.values_mut() {
        instance.executable = None;
        instance.state_root = None;
        instance.resolve_from_path = None;
        instance.credential_id = None;
    }
    source["routing"] = serde_json::to_value(routing)?;
    source["worker_scaling"] = serde_json::to_value(&profile.worker_scaling)?;
    let fields = FIELDS
        .iter()
        .map(|key| {
            Ok((
                key.to_string(),
                source
                    .get(*key)
                    .context("Portable policy field is unavailable")?
                    .clone(),
            ))
        })
        .collect::<Result<_>>()?;
    let mut context = cfg.context.clone();
    context.profiles.retain(|key, _| key == profile_name);
    let result = PortablePolicy {
        fields,
        runtime_rules: runtime_rules(&source)?,
        context,
        llm_model_local: cfg.defaults.llm_model_local.clone(),
        llm_model_cloud: cfg.defaults.llm_model_cloud.clone(),
    };
    let value = serde_json::to_value(&result)?;
    let mut safe = value.clone();
    crate::redact::redact_json_value(&mut safe);
    ensure!(
        safe == value,
        "Portable policy contains a secret; use the executing node's credential configuration"
    );
    Ok(result)
}

pub(super) fn apply(
    policy: &PortablePolicy,
    worker: &GahConfig,
    request: &crate::dispatch::DispatchArgs,
) -> Result<GahConfig> {
    let profile_name = &request.profile;
    ensure!(
        policy.fields.len() == FIELDS.len()
            && FIELDS.iter().all(|key| policy.fields.contains_key(*key)),
        "Incomplete or unsupported portable policy"
    );
    let local = crate::config::get_profile(worker, profile_name)?;
    for (key, expected) in [
        ("repo", &local.repo),
        ("repo_id", &local.repo_id),
        ("provider", &local.provider),
    ] {
        ensure!(
            policy.fields[key].as_str() == Some(expected.as_str()),
            "Worker profile does not match the prepared repository identity"
        );
    }
    let mut value = serde_json::to_value(local)?;
    assert_profile_fields(&value)?;
    let actual_rules = runtime_rules(&value)?;
    ensure!(
        actual_rules.len() == policy.runtime_rules.len()
            && actual_rules
                .keys()
                .all(|key| policy.runtime_rules.contains_key(key)),
        "Unsupported native runtime rules"
    );
    // Explicit backend/model selections cannot use configured fallbacks, even
    // on retries. A heterogeneous node need only match that selected runtime.
    // Auto/unspecified routes must match every potentially reachable runtime.
    let selected = pinned_runtime_rules(request);
    for (key, actual) in actual_rules {
        if selected.is_some_and(|keys| !keys.contains(&key.as_str())) {
            continue;
        }
        ensure!(
            policy.runtime_rules.get(&key) == Some(&actual),
            "Worker runtime rule '{key}' differs from the prepared central policy"
        );
    }
    for (key, field) in &policy.fields {
        value[key] = field.clone();
    }
    let mut profile: Profile = serde_json::from_value(value).map_err(|_| {
        anyhow::anyhow!("Prepared profile policy does not match the supported worker schema")
    })?;
    let local_routing = local.effective_routing(&worker.defaults);
    for (id, portable) in &mut profile.routing.backend_instances {
        ensure!(
            portable.executable.is_none()
                && portable.state_root.is_none()
                && portable.resolve_from_path.is_none()
                && portable.credential_id.is_none(),
            "Portable policy contains node-local runtime configuration"
        );
        let Some(runtime) = local_routing.backend_instances.get(id) else {
            portable.enabled = Some(false);
            continue;
        };
        let matches = |expected: &Option<String>, actual: &Option<String>| {
            expected
                .as_ref()
                .is_none_or(|value| actual.as_ref() == Some(value))
        };
        if portable.runner_kind != runtime.runner_kind
            || !matches(&portable.logical_backend, &runtime.logical_backend)
            || !matches(&portable.quota_pool, &runtime.quota_pool)
            || !matches(&portable.account_label, &runtime.account_label)
            || !matches(&portable.auth_source_label, &runtime.auth_source_label)
        {
            portable.enabled = Some(false);
            continue;
        }
        portable.enabled = Some(portable.enabled() && runtime.enabled());
        portable.logical_backend = runtime.logical_backend.clone();
        portable.quota_pool = runtime.quota_pool.clone();
        portable.account_label = runtime.account_label.clone();
        portable.auth_source_label = runtime.auth_source_label.clone();
        portable.executable = runtime.executable.clone();
        portable.state_root = runtime.state_root.clone();
        portable.resolve_from_path = runtime.resolve_from_path;
        portable.credential_id = runtime.credential_id.clone();
        portable.supported_models = match (
            portable.supported_models.is_empty(),
            runtime.supported_models.is_empty(),
        ) {
            (true, _) => runtime.supported_models.clone(),
            (_, true) => portable.supported_models.clone(),
            _ => {
                let intersection: Vec<_> = portable
                    .supported_models
                    .iter()
                    .filter(|model| runtime.supported_models.contains(model))
                    .cloned()
                    .collect();
                if intersection.is_empty() {
                    portable.enabled = Some(false);
                    portable.supported_models.clone()
                } else {
                    intersection
                }
            }
        };
    }
    profile.notify_command = None;
    profile.manager_wake_autonomy = crate::config::WakeAutonomy::Off;
    profile.policy_path = None; // The verified repository policy is installed by the context loader.
    let mut result = worker.clone();
    result.profiles.clear();
    result.profiles.insert(profile_name.into(), profile);
    result.defaults.routing = RoutingPolicy::default();
    result.defaults.llm_model_local = policy.llm_model_local.clone();
    result.defaults.llm_model_cloud = policy.llm_model_cloud.clone();
    result.defaults.current_manager = None;
    result.defaults.notifications_enabled = Some(false);
    result.context = policy.context.clone();
    Ok(result)
}

fn pinned_runtime_rules(args: &crate::dispatch::DispatchArgs) -> Option<&'static [&'static str]> {
    let model = args.model.as_deref()?.trim();
    if model.is_empty()
        || model.eq_ignore_ascii_case("auto")
        || model.eq_ignore_ascii_case("default")
    {
        return None;
    }
    match crate::config::canonical_backend_name(&args.backend) {
        "codex" => Some(&["codex_args"]),
        "claude" => Some(&["claude_args"]),
        "opencode" => Some(&["opencode_args"]),
        "openhands" => Some(&["openhands_args", "oh_profile"]),
        "vibe" => Some(&["vibe_args"]),
        "hermes" => Some(&["hermes_args"]),
        "agy" | "agy-main" | "agy-second" => Some(&[]),
        _ => None,
    }
}
