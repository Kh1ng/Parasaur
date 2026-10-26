//! Node-local CLI inventory and explicit updates. Provider commands live here,
//! shared by the terminal, dashboard, and scheduled node maintenance.

use crate::backend_kind::BackendKind;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

mod release;
mod state;
#[cfg(test)]
mod tests;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const AGY_RELEASES: &str =
    "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests";

#[derive(clap::Args)]
pub struct AgentsArgs {
    #[command(subcommand)]
    pub command: AgentsCommand,
}

pub(crate) use state::{dispatch_admission, lock as maintenance_lock};

#[derive(clap::Subcommand)]
pub enum AgentsCommand {
    /// Check every configured profile before changing a node-wide CLI.
    CheckUpdatePolicy,
    /// Report installed and latest versions of this node's agent CLIs.
    Versions {
        #[arg(long)]
        json: bool,
    },
    /// Print the update command; execution requires --yes.
    Update {
        backend: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Serialize)]
pub struct AgentVersion {
    pub backend: &'static str,
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub outdated: bool,
    pub source: Option<String>,
    pub update_command: Option<String>,
    pub error: Option<String>,
}

pub fn run(command: AgentsCommand) -> Result<()> {
    match command {
        AgentsCommand::CheckUpdatePolicy => enforce_update_policy(),
        AgentsCommand::Versions { json } => {
            let versions = versions();
            if json {
                println!("{}", serde_json::to_string(&versions)?);
            } else {
                for row in versions {
                    println!(
                        "{}: {} / latest {}{}{}",
                        row.backend,
                        row.installed.as_deref().unwrap_or("unavailable"),
                        row.latest.as_deref().unwrap_or("unknown"),
                        if row.outdated { " (outdated)" } else { "" },
                        row.error.map(|e| format!("; {e}")).unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        AgentsCommand::Update { backend, yes } => update(BackendKind::parse(&backend)?, yes),
    }
}

/// Absolute PATH entries only, excluding npm's bundled SDK binaries.
fn executable(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|dir| {
            dir.is_absolute() && !dir.components().any(|c| c.as_os_str() == "node_modules")
        })
        .map(|dir| dir.join(name))
        .find(|path| crate::runner::resolve::is_executable_path(path))
}

fn output(program: &str, args: &[&str], timeout: Duration) -> Result<String> {
    let path =
        executable(program).with_context(|| format!("{program} is not installed on PATH"))?;
    let mut command = Command::new(path);
    command.args(args);
    let result = crate::runner::process::run_bounded(command, timeout).with_context(|| {
        format!(
            "{program} did not finish within {} seconds",
            timeout.as_secs()
        )
    })?;
    if !result.status.success() {
        bail!("{program} exited unsuccessfully ({})", result.status);
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_string())
}

/// Extract a version token, never a banner or an arbitrary first line.
fn version(text: &str) -> Result<String> {
    let regex = regex::Regex::new(
        r"(?:^|[^\w.])v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)\b",
    )?;
    regex
        .captures(text)
        .map(|c| c[1].to_string())
        .context("Unrecognized version response")
}

fn newer(installed: &str, latest: &str) -> bool {
    let core = |v: &str| {
        v.split(['-', '+'])
            .next()
            .unwrap_or(v)
            .split('.')
            .map(|n| n.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    core(latest) > core(installed)
        || (core(latest) == core(installed) && installed.contains('-') && !latest.contains('-'))
}

fn npm_package(kind: BackendKind, installed: Option<&str>) -> Option<&'static str> {
    match kind {
        BackendKind::Claude => Some("@anthropic-ai/claude-code"),
        BackendKind::Codex => Some("@openai/codex"),
        BackendKind::Opencode => Some(
            if installed
                .and_then(|v| v.split('.').next())
                .and_then(|v| v.parse::<u64>().ok())
                .is_some_and(|major| major >= 2)
            {
                "@opencode/cli"
            } else {
                "opencode-ai"
            },
        ),
        BackendKind::Vibe | BackendKind::Agy | BackendKind::Openhands | BackendKind::Hermes => None,
    }
}

fn source(kind: BackendKind, installed: Option<&str>) -> Option<String> {
    if let Some(package) = npm_package(kind, installed) {
        return Some(format!("npm:{package}"));
    }
    match kind {
        BackendKind::Agy => {
            let os = std::env::consts::OS;
            let os = if os == "macos" { "darwin" } else { os };
            let arch = match std::env::consts::ARCH {
                "x86_64" => "amd64",
                "aarch64" => "arm64",
                other => other,
            };
            let suffix = if cfg!(target_env = "musl") {
                "_musl"
            } else {
                ""
            };
            Some(format!("{AGY_RELEASES}/{os}_{arch}{suffix}.json"))
        }
        BackendKind::Vibe => Some("https://pypi.org/pypi/mistral-vibe/json".into()),
        BackendKind::Claude
        | BackendKind::Codex
        | BackendKind::Opencode
        | BackendKind::Openhands
        | BackendKind::Hermes => None,
    }
}

fn update_command(kind: BackendKind) -> Option<(&'static str, Vec<&'static str>)> {
    match kind {
        BackendKind::Claude => Some(("claude", vec!["update"])),
        BackendKind::Codex => {
            let installed = executable("codex").and_then(|p| p.canonicalize().ok());
            if installed
                .as_ref()
                .is_some_and(|p| p.components().any(|c| c.as_os_str() == "node_modules"))
            {
                Some(("npm", vec!["install", "--global", "@openai/codex@latest"]))
            } else if installed
                .as_ref()
                .is_some_and(|p| p.components().any(|c| c.as_os_str() == "Caskroom"))
            {
                Some(("brew", vec!["upgrade", "--cask", "codex"]))
            } else {
                Some(("codex", vec!["update"]))
            }
        }
        BackendKind::Opencode => Some(("opencode", vec!["upgrade"])),
        BackendKind::Agy => Some(("agy", vec!["update"])),
        BackendKind::Vibe => {
            let brew = executable("vibe")
                .and_then(|p| p.canonicalize().ok())
                .is_some_and(|p| p.components().any(|c| c.as_os_str() == "Cellar"));
            Some(if brew {
                ("brew", vec!["upgrade", "mistral-vibe"])
            } else {
                ("uv", vec!["tool", "upgrade", "mistral-vibe"])
            })
        }
        BackendKind::Openhands | BackendKind::Hermes => None,
    }
}

fn probe(kind: BackendKind) -> AgentVersion {
    let installed = output(kind.as_str(), &["--version"], PROBE_TIMEOUT).and_then(|v| version(&v));
    let source = source(kind, installed.as_deref().ok());
    let latest = source
        .as_deref()
        .map(|s| release::latest(kind, s))
        .transpose();
    let mut errors = Vec::new();
    if let Err(e) = &installed {
        errors.push(e.to_string());
    }
    if let Err(e) = &latest {
        errors.push(e.to_string());
    }
    if source.is_none() {
        errors.push("Latest-version lookup and updates are not supported for this backend".into());
    }
    let installed = installed.ok();
    let latest = latest.ok().flatten();
    AgentVersion {
        backend: kind.as_str(),
        outdated: installed
            .as_deref()
            .zip(latest.as_deref())
            .is_some_and(|(i, l)| newer(i, l)),
        installed,
        latest,
        source,
        update_command: update_command(kind).map(|(p, a)| format!("{p} {}", a.join(" "))),
        error: if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        },
    }
}

/// Probe concurrently; each provider has two independent ten-second ceilings.
pub fn versions() -> Vec<AgentVersion> {
    std::thread::scope(|scope| {
        let probes: Vec<_> = BackendKind::all()
            .into_iter()
            .map(|kind| scope.spawn(move || probe(kind)))
            .collect();
        probes
            .into_iter()
            .map(|p| p.join().expect("agent version probe panicked"))
            .collect()
    })
}

pub fn update(kind: BackendKind, yes: bool) -> Result<()> {
    let (program, args) =
        update_command(kind).context("Updates are not supported for this backend")?;
    let command = format!("{program} {}", args.join(" "));
    println!("Update {}: {command}", kind.as_str());
    if !yes {
        bail!("Review the command, then pass --yes to run it");
    }
    enforce_update_policy()?;
    let _lock = state::lock()?;
    let mut record = state::UpdateRecord::new(kind.as_str(), command);
    state::save(&record)?;
    let result = (|| {
        record.before =
            Some(output(kind.as_str(), &["--version"], PROBE_TIMEOUT).and_then(|v| version(&v))?);
        state::save(&record)?;
        let source = source(kind, record.before.as_deref()).context("Latest release unknown")?;
        release::latest(kind, &source).context("Latest release unknown; refusing installation")?;
        // Admission stays closed from this check until the installer exits.
        crate::work_claim::ensure_node_idle()?;
        output(program, &args, Duration::from_secs(600))?;
        output(kind.as_str(), &["--version"], PROBE_TIMEOUT).and_then(|v| version(&v))
    })();
    record.finish(&result);
    state::save(&record)?;
    result.map(|after| println!("{} is now {after}", kind.as_str()))
}

/// CLI changes affect every profile on this node, so choosing a permissive
/// profile cannot bypass a read-only profile's policy. Missing config fails closed.
fn enforce_update_policy() -> Result<()> {
    let config = crate::config::load(None)?;
    for profile in config.profiles.values() {
        crate::policy::enforce_policy(profile, "update-agent-cli")?;
    }
    Ok(())
}
