use crate::job_kind::{JobFamily, JobKind};
use crate::provider_kind::{ProviderKind, UnknownProviderKind};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

mod backend_instances;
mod merge_policy;
mod node_capacity;
pub use backend_instances::*;
pub use merge_policy::MergePolicy;
pub use node_capacity::NodeCapacitySettings;
mod backend_paths;
mod issue_intake;
pub use issue_intake::IssueIntakeMode;
mod publishing;
pub use publishing::{IssueClaimMode, IssueClaimPolicy, PublishingPolicy};
mod delivery;
pub use delivery::DeliveryMode;
mod autonomy;
pub use autonomy::WakeAutonomy;
mod external_credential_scopes;
pub use external_credential_scopes::ExternalCredentialScope;
mod loading;
mod orchestration;
pub use loading::load;
mod routing_policy;
mod spend_budget;
pub use spend_budget::{SpendBudget, SpendCostBasis};
mod worker_scaling;
pub use orchestration::OrchestrationMode;
use routing_policy::merge_routing_policy;
pub use routing_policy::{CandidateConfig, IssueBudget, RoutingPolicy, TaskRoutingRule};
mod default_paths;
pub use default_paths::{default_config_dir, default_data_root, effective_worktree_base};
pub use worker_scaling::WorkerScaling;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GahConfig {
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,
    #[serde(default)]
    pub context: crate::context::ContextConfig,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
#[serde(default)]
pub struct Defaults {
    /// Cloud modes require PARASAUR_SUPABASE_URL and PARASAUR_SUPABASE_ANON_KEY in the environment.
    pub orchestration_mode: OrchestrationMode,
    /// None retains factory behavior for configurations predating module selection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factory_enabled: Option<bool>,
    pub artifact_root: String,
    pub worktree_base: String,
    pub llm_base_url: String,
    pub llm_model_local: String,
    pub llm_model_cloud: String,
    pub routing: RoutingPolicy,
    pub node_capacity: NodeCapacitySettings,
    /// Global manager CLI ("claude" | "codex" | "hermes") across all projects.
    /// `Profile::manager_wake_autonomy` uses it for notify-worthy events;
    /// `None`/unrecognized means no wake, even when profile autonomy is enabled.
    pub current_manager: Option<String>,
    /// See `crate::network_exposure` module docs (issue #879).
    pub network_exposure: crate::network_exposure::NetworkExposureLevel,
    /// See `crate::network_exposure` module docs.
    pub lan_cidrs: Vec<String>,
    /// See `crate::network_exposure` module docs.
    #[serde(default = "crate::network_exposure::default_tailscale_cidr")]
    pub tailscale_cidr: Option<String>,
    pub registry_central_url: Option<String>,
    /// Host responsibility. Worker services keep execution local and central state remote.
    pub node_role: crate::node_role::NodeRole,
    pub registry_preflight_mode: crate::fleet_preflight::PreflightMode,
    /// Issue #1454: the notification-channel module switch. `None` (a config
    /// from before the switch) means on exactly when a channel is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notifications_enabled: Option<bool>,
    /// Issue #653: where notify-worthy events also go. Credentials never live
    /// here; see `crate::notify_channels::credential`.
    pub notification_channel: crate::notify_channels::NotificationChannel,
    /// Telegram chat to deliver to when the channel is `telegram`.
    pub telegram_chat_id: Option<String>,
}

impl Defaults {
    pub fn llm_base_url(&self) -> String {
        std::env::var("LLM_BASE_URL").unwrap_or_else(|_| self.llm_base_url.clone())
    }
    pub fn llm_api_key(&self) -> String {
        std::env::var("LLM_API_KEY").unwrap_or_default()
    }
    pub fn llm_model(&self, cloud: bool) -> String {
        if let Ok(m) = std::env::var("LLM_MODEL") {
            return m;
        }
        if cloud {
            self.llm_model_cloud.clone()
        } else {
            self.llm_model_local.clone()
        }
    }

    pub fn ledger_path(&self) -> PathBuf {
        if let Ok(path) = std::env::var("PARASAUR_LEDGER_PATH") {
            return PathBuf::from(path);
        }
        if !self.artifact_root.trim().is_empty() {
            return PathBuf::from(self.artifact_root.trim()).join("ledger.jsonl");
        }
        default_config_dir().join("ledger.jsonl")
    }

    /// TICKET-072: separate append-only log from `ledger.jsonl` (never
    /// rewrites dispatch history), same directory/override convention as
    /// `PARASAUR_LEDGER_PATH`.
    pub fn reconciliation_path(&self) -> PathBuf {
        if let Ok(path) = std::env::var("PARASAUR_RECONCILIATION_PATH") {
            return PathBuf::from(path);
        }
        if !self.artifact_root.trim().is_empty() {
            return PathBuf::from(self.artifact_root.trim()).join("reconciliation.jsonl");
        }
        default_config_dir().join("reconciliation.jsonl")
    }

    /// TICKET-083: append-only controller event stream, same convention as
    /// `PARASAUR_LEDGER_PATH`/`PARASAUR_RECONCILIATION_PATH`.
    pub fn events_path(&self) -> PathBuf {
        if let Ok(path) = std::env::var("PARASAUR_EVENTS_PATH") {
            return PathBuf::from(path);
        }
        if !self.artifact_root.trim().is_empty() {
            return PathBuf::from(self.artifact_root.trim()).join("events.jsonl");
        }
        default_config_dir().join("events.jsonl")
    }

    /// Directory `manager_wake_autonomy` audit logs are written under, same
    /// override convention as `PARASAUR_LEDGER_PATH`/`PARASAUR_EVENTS_PATH`.
    pub fn manager_wake_log_dir(&self) -> PathBuf {
        if let Ok(path) = std::env::var("PARASAUR_MANAGER_WAKE_LOG_DIR") {
            return PathBuf::from(path);
        }
        if !self.artifact_root.trim().is_empty() {
            return PathBuf::from(self.artifact_root.trim()).join("manager-wake-logs");
        }
        default_config_dir().join("manager-wake-logs")
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Profile {
    pub display_name: String,
    pub repo_id: String,
    pub provider: String,
    pub repo: String,
    pub local_path: String,
    pub artifact_root: String,
    pub default_target_branch: String,
    #[serde(default)]
    pub provider_api_base: Option<String>,
    #[serde(default)]
    pub provider_project_id: Option<String>,
    /// OpenHands profile name (~/.openhands/profiles/<name>.json). Overrides default LLM config.
    #[serde(default)]
    pub oh_profile: Option<String>,
    /// Hermes/OpenHands resolution failures: fail_closed (default) or proceed_without_skills.
    #[serde(default)]
    pub skill_resolve_failure_policy: crate::skill_bindings::ResolveFailurePolicy,
    /// Extra OpenHands CLI args; `--skills` is ignored in favor of central bindings.
    #[serde(default)]
    pub openhands_args: Vec<String>,
    /// Extra CLI args appended to `codex exec` for invariant non-model flags.
    #[serde(default)]
    pub codex_args: Vec<String>,
    /// Optional absolute/relative path to the Codex CLI executable.
    #[serde(default)]
    pub codex_path: Option<String>,
    /// Extra CLI args appended to `claude -p` (e.g. `--allowedTools Edit,Write,Bash`)
    #[serde(default)]
    pub claude_args: Vec<String>,
    /// Optional absolute/relative path to the Claude CLI executable.
    #[serde(default)]
    pub claude_path: Option<String>,
    /// Optional absolute/relative path to the Antigravity CLI executable.
    #[serde(default)]
    pub agy_path: Option<String>,
    /// Extra CLI args appended to `vibe -p` (e.g. `--max-turns 40 --max-price 2`).
    /// Applied to worker/fix and review execution.
    #[serde(default)]
    pub vibe_args: Vec<String>,
    /// Optional absolute/relative path to the Mistral Vibe CLI executable.
    #[serde(default)]
    pub vibe_path: Option<String>,
    /// Role-agnostic `opencode run` args for worker/fix and review execution;
    /// GAH owns and filters agent and model selection.
    #[serde(default)]
    pub opencode_args: Vec<String>,
    /// Optional absolute/relative path to the OpenCode CLI executable.
    #[serde(default)]
    pub opencode_path: Option<String>,
    /// Extra Hermes CLI args; `--skills` is ignored in favor of central bindings.
    #[serde(default)]
    pub hermes_args: Vec<String>,
    /// Optional absolute/relative path to the Hermes CLI executable.
    #[serde(default)]
    pub hermes_path: Option<String>,
    /// How long OpenCode can go without a durable worktree change before GAH
    /// considers it stalled and kills it, in seconds. OpenCode's narration
    /// and malformed tool-call output deliberately do not reset this window:
    /// only repository progress does. This retains an activity-based guard
    /// rather than imposing a flat dispatch deadline. Defaults to 300s when
    /// unset.
    #[serde(default)]
    pub opencode_idle_timeout_seconds: Option<u64>,
    /// Per-model override for `opencode_idle_timeout_seconds`, keyed by the
    /// exact model name passed to `opencode --model` (e.g.
    /// "litellm-lan/qwen3.6:35b-a3b"). Some models routed through opencode
    /// are genuinely slow-but-working (a self-hosted litellm proxy) rather
    /// than hung, while others (a free-tier rate-limited model) hang with
    /// zero output and should be killed fast -- one flat idle timeout for
    /// every opencode model is too coarse. Falls back to the flat
    /// `opencode_idle_timeout_seconds` when no entry matches. Same pattern
    /// as `agy_print_timeout_seconds` below.
    #[serde(default)]
    pub opencode_idle_timeout_seconds_by_model: HashMap<String, u64>,
    /// Per-backend/model concurrency caps keyed by `"{backend}/{model}"`.
    /// Missing keys are unlimited. Enforcement is process-local; the profile
    /// lock guarantees one dispatching GAH process per profile.
    #[serde(default)]
    pub max_concurrent_per_model: HashMap<String, u32>,
    /// OpenHands idle-output timeout in seconds. Defaults to 300s; active
    /// worktree progress keeps a quiet process alive.
    #[serde(default)]
    pub openhands_idle_timeout_seconds: Option<u64>,
    /// How long Vibe's own log output can go quiet before GAH considers it
    /// stalled and kills it, in seconds. Same rationale and mechanism as
    /// `opencode_idle_timeout_seconds` -- added after a live dispatch hung
    /// for 15+ minutes with vibe as the backend and `run_vibe_with_executable`
    /// had zero supervision of any kind. Defaults to 300s when unset.
    #[serde(default)]
    pub vibe_idle_timeout_seconds: Option<u64>,
    /// How long Codex's own log output can go quiet before GAH considers it
    /// stalled and kills it, in seconds. Same rationale and mechanism as
    /// `opencode_idle_timeout_seconds`. Defaults to 300s when unset.
    #[serde(default)]
    pub codex_idle_timeout_seconds: Option<u64>,
    /// How long Claude Code's own log output can go quiet before GAH
    /// considers it stalled and kills it, in seconds. Same rationale and
    /// mechanism as `opencode_idle_timeout_seconds`. Defaults to 300s when
    /// unset.
    #[serde(default)]
    pub claude_idle_timeout_seconds: Option<u64>,
    /// How long Hermes's own log output can go quiet before GAH considers it
    /// stalled and kills it, in seconds. Same rationale and mechanism as
    /// `opencode_idle_timeout_seconds`. Defaults to 300s when unset.
    #[serde(default)]
    pub hermes_idle_timeout_seconds: Option<u64>,
    /// How many tickets `gah loop` may execute concurrently for this profile.
    /// The native recurring loop owns this worker pool; the shell supervisor
    /// is only a compatibility launcher. Defaults to 1 when unset.
    #[serde(default)]
    pub max_parallel_workers: Option<u32>,
    /// Open managed PRs/MRs plus pre-publication dispatches allowed before
    /// implementation intake pauses. Defaults to `max_parallel_workers`.
    #[serde(default)]
    pub max_open_managed_mrs: Option<u32>,
    /// Automatic and manual growth past the two limits above.
    #[serde(default, skip_serializing_if = "WorkerScaling::is_default")]
    pub worker_scaling: WorkerScaling,
    /// HOME override for the `agy-second` backend name only -- a distinct
    /// authenticated Antigravity account/quota pool from the default `agy`
    /// backend, which otherwise runs under the process's real $HOME. Same
    /// executable (`agy_path`), different account state directory.
    #[serde(default)]
    pub agy_second_home: Option<String>,
    /// Per-model override for AGY's own `--print-timeout` (default 5m0s in
    /// the `agy` CLI itself). Keyed by the exact AGY model name (e.g.
    /// "Gemini 3.5 Flash (Medium)"). This is now an outer safety backstop
    /// only -- the primary enforcement is `agy_idle_timeout_seconds` below,
    /// which kills based on whether AGY is still producing output, not a
    /// flat wall-clock budget. AGY is the only backend GAH invokes that
    /// exposes a print-timeout flag today, so this stays scoped to AGY
    /// rather than a generic cross-backend timeout abstraction.
    #[serde(default)]
    pub agy_print_timeout_seconds: HashMap<String, u64>,
    /// How long AGY's own log output can go quiet before GAH considers it
    /// stalled and kills it, in seconds. Deliberately not per-model: a
    /// working model of any speed should produce *some* log output
    /// periodically as it takes actions, so a flat idle threshold is the
    /// right granularity here (unlike agy_print_timeout_seconds, which is
    /// about total budget and genuinely varies by model). Defaults to 120s
    /// when unset (see `Profile::agy_idle_timeout_seconds`).
    #[serde(default)]
    pub agy_idle_timeout_seconds: Option<u64>,
    /// Optional shell command that GAH pipes a one-line notification message to
    /// (via stdin) on key controller/dispatch events (MR created, human required,
    /// review verdict, terminal dispatch failure). When unset GAH produces no
    /// notification output at all. Notification failures are always swallowed and
    /// logged to stderr -- they never fail the operation.
    #[serde(default)]
    pub notify_command: Option<String>,
    /// How much a woken manager agent is allowed to do on its own for this
    /// profile, when a notify-worthy event fires. See `WakeAutonomy` and
    /// `Defaults::current_manager`. Defaults to `Off` -- an operator opts a
    /// specific profile into this.
    #[serde(default)]
    pub manager_wake_autonomy: WakeAutonomy,
    /// Delivery mode for work results: `pr` (default) | `handoff`.
    #[serde(default)]
    pub delivery_mode: DeliveryMode,
    /// Path to a policy TOML file (see gah policy-check). When set, dispatch
    /// enforces permissions before provisioning any worktree.
    #[serde(default)]
    pub policy_path: Option<String>,
    /// Optional path to a KEY=VALUE env file sourced before running any backend
    /// in dev mode (default). Contains dev/api keys, never prod credentials.
    #[serde(default)]
    pub env_file: Option<String>,
    /// Path to a production KEY=VALUE env file. Only loaded when --prod is passed
    /// to dispatch. Keeps prod credentials isolated from dev runs.
    #[serde(default)]
    pub env_file_prod: Option<String>,
    /// Commands run in the worktree after each agent attempt; all must pass before commit/push.
    /// Example: ["cargo test --quiet", "cargo clippy -- -D warnings"]
    #[serde(default)]
    pub validation_commands: Vec<String>,
    /// Best-effort mechanical fixups run before validation on every attempt.
    /// Example: ["cargo fmt"].
    #[serde(default)]
    pub auto_fix_commands: Vec<String>,
    #[serde(default)]
    pub test_file_patterns: Vec<String>,
    /// Explicit case-insensitive markers for known baseline failures.
    #[serde(default)]
    pub known_baseline_failure_markers: Vec<String>,
    /// Model override for `improve`/`fix` mode (heavy lifting)
    #[serde(default)]
    pub model_improve: Option<String>,
    /// Model override for `pm` mode (ticket decomposition, cheap/fast)
    #[serde(default)]
    pub model_pm: Option<String>,
    /// Model override for `review` mode
    #[serde(default)]
    pub model_review: Option<String>,
    /// Review idle timeout in seconds (default 300).
    #[serde(default)]
    pub review_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub review_hard_timeout_seconds: Option<u64>,
    /// Per-command validation timeout in seconds (default 300).
    #[serde(default)]
    pub validation_timeout_seconds: Option<u64>,
    #[serde(default)]
    pub routing: RoutingPolicy,
    /// TICKET-128: per-profile policy for human-facing repository messaging.
    /// Independent of reviewer routing and merge authorization; defaults to
    /// allowing PR/MR text, commit messages, and issue comments.
    #[serde(default)]
    pub publishing: PublishingPolicy,
    #[serde(default)]
    pub pacing: crate::quota::PacingConfig,
    /// TICKET-158: per-profile retention window (days) for `gah prune`.
    /// High-churn self-hosting profiles can prune aggressively (e.g. 3-7)
    /// while low-churn profiles keep the 30-day default. The CLI
    /// `--older-than` flag overrides this per invocation.
    #[serde(default)]
    pub prune_older_than_days: Option<u64>,
    /// Idle chat sessions are archived by the daily chat-maintenance sweep
    /// after this many days without a completed turn. Unlike worktree prune,
    /// archive preserves dirty work as a patch and always keeps the branch.
    #[serde(default)]
    pub chat_session_idle_days: Option<u64>,
    #[serde(default)]
    pub external_credential_scopes: HashMap<String, ExternalCredentialScope>,
}

fn normalize_repo_path(repo: &str) -> String {
    let repo = repo.trim_matches('/');
    if repo.ends_with(".git") {
        repo.to_string()
    } else {
        format!("{}.git", repo)
    }
}

/// Canonicalizes `--backend` aliases that execute the same backend but were
/// being recorded under their literal alias string, producing duplicate
/// cards for one backend on the quota page (e.g. "openhands" and
/// "cloud-coder" both run OpenHands via `runner::backend_command_name`, but
/// only "openhands" was ever normalized there -- the raw CLI string was
/// still what got written to `requested_backend`/`effective_backend` and
/// from there into the ledger). Applied both where new dispatches are
/// routed (dispatch.rs) and when grouping the ledger for the quota page
/// (ledger/mod.rs), so it also merges pre-existing historical entries recorded
/// under the old alias rather than only preventing new duplicates.
/// Deliberately does NOT touch "auto": that backend's *effective* backend
/// is resolved dynamically per-attempt by `routing::decide`, not a fixed
/// alias, so it must pass through unchanged.
pub fn canonical_backend_name(name: &str) -> &str {
    if name == "cloud-coder" {
        "openhands"
    } else {
        name
    }
}

pub fn default_config_path() -> PathBuf {
    default_config_dir().join("config.toml")
}

pub fn resolve_config_path(config_path: Option<&str>) -> PathBuf {
    config_path
        .map(PathBuf::from)
        .or_else(|| std::env::var("PARASAUR_CONFIG").ok().map(PathBuf::from))
        .unwrap_or_else(default_config_path)
}

/// TICKET-106: shared canonical routing policy, inherited by every repo's
/// config unless explicitly overridden. Separate file from the per-repo
/// config (`PARASAUR_CANONICAL_CONFIG` override, else `~/.config/gah/canonical.toml`
/// -- same `~/.config/gah/` convention as the default repo config path).
pub fn canonical_config_path() -> PathBuf {
    #[cfg(test)]
    {
        crate::test_support::canonical_config_path()
    }
    #[cfg(not(test))]
    std::env::var("PARASAUR_CANONICAL_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_config_dir().join("canonical.toml"))
}

#[derive(Debug, Deserialize, Default)]
struct CanonicalConfig {
    #[serde(default)]
    routing: RoutingPolicy,
}

/// `Ok(None)` when no canonical file exists (legacy standalone behavior).
/// Malformed canonical config is a hard error, never silently ignored --
/// an operator's shared policy must not silently fail to apply.
fn load_canonical_routing() -> Result<Option<RoutingPolicy>> {
    let path = canonical_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let canonical: CanonicalConfig =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(canonical.routing))
}

pub fn check_profile_candidate_model_consistency(
    defaults: &Defaults,
    profile: &Profile,
) -> Result<(), Vec<String>> {
    let routing = profile.effective_routing(defaults);
    let candidates = routing.labeled_candidates();
    let mut errors = routing.allowed_model_errors();
    for (label, candidate) in candidates {
        // An allow-list entry without a model admits any model on its backend.
        if label == "allowed_model" && candidate.model.is_none() {
            continue;
        }
        let args = match candidate.backend.as_str() {
            "codex" => &profile.codex_args,
            "opencode" => &profile.opencode_args,
            "claude" => &profile.claude_args,
            _ => continue,
        };
        if let Some(pinned) =
            crate::runner::extract_model_from_backend_args(&candidate.backend, args)
        {
            if candidate.model.as_deref() != Some(&pinned) {
                errors.push(format!(
                    "candidate {} (backend '{}', label '{}') has mismatch with profile backend args model pin '{}'",
                    label,
                    candidate.backend,
                    candidate.model.as_deref().unwrap_or("None"),
                    pinned
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn get_profile<'a>(config: &'a GahConfig, name: &str) -> Result<&'a Profile> {
    config.profiles.get(name).ok_or_else(|| {
        let mut names: Vec<&str> = config.profiles.keys().map(String::as_str).collect();
        names.sort_unstable();
        anyhow::anyhow!(
            "profile '{}' not found; available: {}",
            name,
            names.join(", ")
        )
    })
}

/// Save the config back to the TOML file
pub fn save(config: &GahConfig, path: Option<&str>) -> Result<()> {
    config.defaults.node_capacity.validate()?;
    let target_path = resolve_config_path(path);

    // Ensure parent directory exists
    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent).context("creating config directory")?;
    }

    let toml_string = toml::to_string(config).context("serializing config to TOML")?;
    std::fs::write(&target_path, toml_string).context("writing config file")?;
    Ok(())
}

/// Add a new profile to the config
pub fn add_profile(config: &mut GahConfig, name: &str, profile: Profile) -> Result<()> {
    if config.profiles.contains_key(name) {
        anyhow::bail!("profile '{}' already exists", name);
    }
    config.profiles.insert(name.to_string(), profile);
    Ok(())
}

/// Remove a profile from the config
pub fn remove_profile(config: &mut GahConfig, name: &str) -> Result<()> {
    if !config.profiles.contains_key(name) {
        anyhow::bail!("profile '{}' not found", name);
    }
    config.profiles.remove(name);
    Ok(())
}

/// Get a mutable reference to a profile for in-place modification
pub fn get_profile_mut<'a>(config: &'a mut GahConfig, name: &str) -> Result<&'a mut Profile> {
    if !config.profiles.contains_key(name) {
        let names: Vec<String> = config.profiles.keys().cloned().collect();
        anyhow::bail!(
            "profile '{}' not found; available: {}",
            name,
            names.join(", ")
        );
    }
    Ok(config.profiles.get_mut(name).unwrap())
}

#[cfg(test)]
pub mod tests;
