//! Profile and workspace configuration command grammar.
use clap::Subcommand;

// Clap derives the flat profile flags directly; this command is parsed once.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub enum ProfileCommands {
    /// List all profiles in config
    List {
        #[arg(long)]
        config: Option<String>,
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Show details for a profile
    Show {
        name: String,
        #[arg(long)]
        config: Option<String>,
    },
    /// Add a new profile
    Add {
        /// Profile name (used as the TOML table key under [profiles])
        name: String,
        #[arg(long)]
        display_name: String,
        #[arg(long)]
        repo_id: String,
        #[arg(long)]
        provider: String,
        #[arg(long)]
        repo: String,
        #[arg(long)]
        local_path: String,
        #[arg(long)]
        artifact_root: String,
        #[arg(long, default_value = "main")]
        default_target_branch: String,
        #[arg(long)]
        provider_api_base: Option<String>,
        #[arg(long)]
        provider_project_id: Option<String>,
        #[arg(long = "config", visible_alias = "config-path")]
        config_path: Option<String>,
        /// Extra CLI args for openhands
        #[arg(long, value_delimiter = ',')]
        openhands_args: Vec<String>,
        /// Extra CLI args for codex
        #[arg(long, value_delimiter = ',')]
        codex_args: Vec<String>,
        /// Path to codex executable
        #[arg(long)]
        codex_path: Option<String>,
        /// Extra CLI args for claude
        #[arg(long, value_delimiter = ',')]
        claude_args: Vec<String>,
        /// Path to claude executable
        #[arg(long)]
        claude_path: Option<String>,
        /// Path to agy executable
        #[arg(long)]
        agy_path: Option<String>,
        /// Extra CLI args for vibe
        #[arg(long, value_delimiter = ',')]
        vibe_args: Vec<String>,
        /// Path to vibe executable
        #[arg(long)]
        vibe_path: Option<String>,
        /// Extra CLI args for opencode
        #[arg(long, value_delimiter = ',')]
        opencode_args: Vec<String>,
        /// Path to opencode executable
        #[arg(long)]
        opencode_path: Option<String>,
        /// Extra CLI args for hermes
        #[arg(long, value_delimiter = ',')]
        hermes_args: Vec<String>,
        /// Path to hermes executable
        #[arg(long)]
        hermes_path: Option<String>,
        /// HOME override for agy-second backend
        #[arg(long)]
        agy_second_home: Option<String>,
        /// Notification command
        #[arg(long)]
        notify_command: Option<String>,
        /// Policy path
        #[arg(long)]
        policy_path: Option<String>,
        /// Dev env file
        #[arg(long)]
        env_file: Option<String>,
        /// Production env file
        #[arg(long)]
        env_file_prod: Option<String>,
        /// Validation commands
        #[arg(long, value_delimiter = ',')]
        validation_commands: Vec<String>,
        /// Auto-fix commands
        #[arg(long, value_delimiter = ',')]
        auto_fix_commands: Vec<String>,
        /// How many tickets `gah loop` may execute concurrently for this
        /// profile (defaults to 1). Exposed in the dashboard Settings UI.
        #[arg(long)]
        max_parallel_workers: Option<u32>,
        /// Maximum open/in-flight managed PRs or MRs before implementation
        /// intake pauses and lifecycle work drains.
        #[arg(long)]
        max_open_managed_mrs: Option<u32>,
        /// Timeout in seconds for each validation command (defaults to 300).
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        validation_timeout_seconds: Option<u64>,
        /// Manager-wake autonomy for this profile: off | review_only | full.
        #[arg(long)]
        manager_wake_autonomy: Option<String>,
        /// Delivery mode for work results: pr (default) | handoff.
        #[arg(long)]
        delivery_mode: Option<String>,
    },
    /// Set/Update fields of an existing profile
    Set {
        name: String,
        #[arg(long)]
        display_name: Option<String>,
        #[arg(long)]
        repo_id: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        local_path: Option<String>,
        #[arg(long)]
        artifact_root: Option<String>,
        #[arg(long)]
        default_target_branch: Option<String>,
        #[arg(long)]
        provider_api_base: Option<String>,
        #[arg(long)]
        provider_project_id: Option<String>,
        #[arg(long = "config", visible_alias = "config-path")]
        config_path: Option<String>,
        #[arg(long, value_delimiter = ',')]
        openhands_args: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        codex_args: Vec<String>,
        #[arg(long)]
        codex_path: Option<String>,
        #[arg(long, value_delimiter = ',')]
        claude_args: Vec<String>,
        #[arg(long)]
        claude_path: Option<String>,
        #[arg(long)]
        agy_path: Option<String>,
        #[arg(long, value_delimiter = ',')]
        vibe_args: Vec<String>,
        #[arg(long)]
        vibe_path: Option<String>,
        #[arg(long, value_delimiter = ',')]
        opencode_args: Vec<String>,
        #[arg(long)]
        opencode_path: Option<String>,
        #[arg(long, value_delimiter = ',')]
        hermes_args: Vec<String>,
        #[arg(long)]
        hermes_path: Option<String>,
        #[arg(long)]
        agy_second_home: Option<String>,
        #[arg(long)]
        notify_command: Option<String>,
        #[arg(long)]
        policy_path: Option<String>,
        #[arg(long)]
        env_file: Option<String>,
        #[arg(long)]
        env_file_prod: Option<String>,
        #[arg(long, value_delimiter = ',')]
        validation_commands: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        auto_fix_commands: Vec<String>,
        /// How many tickets `gah loop` may execute concurrently for this
        /// profile (defaults to 1). Exposed in the dashboard Settings UI.
        #[arg(long)]
        max_parallel_workers: Option<u32>,
        /// Maximum open/in-flight managed PRs or MRs before implementation
        /// intake pauses and lifecycle work drains.
        #[arg(long)]
        max_open_managed_mrs: Option<u32>,
        /// Timeout in seconds for each validation command (defaults to 300).
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        validation_timeout_seconds: Option<u64>,
        /// Manager-wake autonomy for this profile: off | review_only | full.
        #[arg(long)]
        manager_wake_autonomy: Option<String>,
        /// Delivery mode for work results: pr | handoff.
        #[arg(long)]
        delivery_mode: Option<String>,
        /// Automatic worker scaling from quota headroom: on | off (see `WorkerScaling`).
        #[arg(long)]
        worker_scaling: Option<String>,
        /// Most workers automatic scaling may reach (default: twice the baseline).
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        worker_scaling_max_workers: Option<u32>,
        /// Extra concurrent runs a model gets while it has quota headroom.
        #[arg(long)]
        worker_scaling_extra_per_model: Option<u32>,
        /// Percent every fresh quota window must still have for a model to scale.
        #[arg(long)]
        worker_scaling_min_remaining_percent: Option<f64>,
        /// Add this many workers now; replaces an earlier boost (`--clear worker_boost` ends it).
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        boost_workers: Option<u32>,
        /// Give the boost to one `backend/model` instead of every capped model.
        #[arg(long, requires = "boost_workers")]
        boost_model: Option<String>,
        /// End the boost after this many hours (default: until cleared).
        #[arg(long, requires = "boost_workers")]
        boost_hours: Option<f64>,
        /// Hold approved schema/API contract changes for human review.
        #[arg(long)]
        hold_contract_changes: Option<bool>,
        /// Switch a backend's model in every routing list: `backend/old=new`.
        /// The old model's concurrency cap and boost move with it. Repeatable.
        #[arg(long)]
        agent_model: Vec<String>,
        /// Set native reasoning for a backend: backend=effort.
        #[arg(long)]
        agent_effort: Vec<String>,
        /// Cap one model's concurrent runs: `backend/model=count`, or remove
        /// its cap with a count of 0. Repeat for several models;
        /// `--clear max_concurrent_per_model` removes every cap.
        #[arg(long)]
        max_concurrent: Vec<String>,
        /// Clear the specified field(s) - for fields that support it
        #[arg(long, value_delimiter = ',')]
        clear: Vec<String>,
    },
    /// Remove a profile
    Remove {
        name: String,
        #[arg(long = "config", visible_alias = "config-path")]
        config_path: Option<String>,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
}

