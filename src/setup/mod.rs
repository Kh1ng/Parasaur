//! Guided first install (`gah setup`).
//!
//! - `requirements` is the one list of what each feature needs.
//! - `host` reads the machine; `project` turns a checkout into a profile.
//! - `wizard` asks, checks, offers, installs, and summarizes.

pub mod host;
pub mod project;
pub mod requirements;
pub mod wizard;

use anyhow::{bail, Context, Result};
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Prompts on the terminal. Hidden input turns echo off with `stty`.
pub struct TerminalPrompter;

fn read_line() -> Result<String> {
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line)? == 0 {
        bail!("Input ended before setup finished.");
    }
    Ok(line.trim().to_string())
}

impl wizard::Prompter for TerminalPrompter {
    fn say(&mut self, line: &str) {
        println!("{line}");
    }

    fn choose(&mut self, question: &str, options: &[String], default: usize) -> Result<usize> {
        println!("{question}");
        for (index, option) in options.iter().enumerate() {
            println!(
                "  {}) {option}{}",
                index + 1,
                if index == default { "  [default]" } else { "" }
            );
        }
        loop {
            print!("Choose 1-{} [{}]: ", options.len(), default + 1);
            std::io::stdout().flush()?;
            let answer = read_line()?;
            if answer.is_empty() {
                return Ok(default);
            }
            match answer.parse::<usize>() {
                Ok(choice) if (1..=options.len()).contains(&choice) => return Ok(choice - 1),
                _ => println!("Enter a number from the list."),
            }
        }
    }

    fn confirm(&mut self, question: &str, default: bool) -> Result<bool> {
        loop {
            print!("{question} [{}] ", if default { "Y/n" } else { "y/N" });
            std::io::stdout().flush()?;
            match read_line()?.to_lowercase().as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => println!("Answer y or n."),
            }
        }
    }

    fn text(&mut self, question: &str, default: Option<&str>) -> Result<String> {
        match default {
            Some(default) => print!("{question} [{default}]: "),
            None => print!("{question}: "),
        }
        std::io::stdout().flush()?;
        let answer = read_line()?;
        Ok(if answer.is_empty() {
            default.unwrap_or_default().to_string()
        } else {
            answer
        })
    }

    fn secret(&mut self, question: &str) -> Result<String> {
        print!("{question}: ");
        std::io::stdout().flush()?;
        let echo = |on: bool| {
            let _ = Command::new("stty")
                .arg(if on { "echo" } else { "-echo" })
                .stdin(Stdio::inherit())
                .status();
        };
        echo(false);
        let answer = read_line();
        echo(true);
        println!();
        answer
    }
}

/// The real machine: shell commands with the terminal attached, HTTP through
/// curl (credentials in its stdin config, never argv), and the GAH config.
pub struct SystemEffects;

impl wizard::Effects for SystemEffects {
    fn run(&mut self, command: &str, cwd: Option<&Path>, env: &[(&str, String)]) -> bool {
        println!("$ {command}");
        let mut process = Command::new("sh");
        process.arg("-c").arg(command);
        if let Some(cwd) = cwd {
            process.current_dir(cwd);
        }
        for (key, value) in env {
            process.env(key, value);
        }
        process
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn http(
        &mut self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        body: Option<&str>,
    ) -> Option<u16> {
        crate::curl_http::request(method, url, body, bearer, 10)
            .ok()
            .map(|response| response.status)
    }

    fn profile_exists(&mut self, profile: &str) -> bool {
        std::fs::read_to_string(crate::config::resolve_config_path(None))
            .map(|config| config.contains(&format!("[profiles.{profile}]")))
            .unwrap_or(false)
    }

    fn add_profile(&mut self, args: crate::init::InitArgs) -> Result<()> {
        crate::init::run(args)
    }

    /// Sets `permissionMode` in the file the server reads (its
    /// GAH_MANAGER_CHAT_SETTINGS_PATH, else `config/` in the checkout it runs
    /// from), keeping every other setting, and replaces the file atomically.
    fn set_chat_permissions(&mut self, source: &Path, mode: wizard::ChatPermissions) -> Result<()> {
        let path = std::env::var_os("GAH_MANAGER_CHAT_SETTINGS_PATH")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| source.join("config/manager-chat-settings.json"));
        let mut settings = match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes)
                    .with_context(|| {
                        format!(
                            "{} is not a JSON object; fix or remove it and run setup again",
                            path.display()
                        )
                    })?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::Map::new(),
            Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
        };
        settings.insert("permissionMode".into(), mode.stored().into());
        let parent = path.parent().context("settings path has no parent")?;
        std::fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut temp, &settings)?;
        temp.as_file().sync_all()?;
        temp.persist(&path)
            .map_err(|error| error.error)
            .with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }

    fn notification_settings(&mut self) -> wizard::NotificationSettings {
        let defaults = load_or_default_config()
            .map(|cfg| cfg.defaults)
            .unwrap_or_default();
        wizard::NotificationSettings {
            enabled: crate::notify_channels::module_enabled(&defaults),
            channel: defaults.notification_channel,
            telegram_chat_id: defaults.telegram_chat_id.clone(),
            // Only the private file counts: this shell's environment is not
            // what the server and watchdog services read.
            credential_saved: matches!(
                crate::notify_channels::credential(defaults.notification_channel),
                Some((_, crate::notify_channels::CredentialSource::File))
            ),
        }
    }

    fn save_notification_settings(
        &mut self,
        settings: &wizard::NotificationSettings,
        credential: Option<&str>,
    ) -> Result<()> {
        let mut cfg = load_or_default_config()?;
        cfg.defaults.notifications_enabled = Some(settings.enabled);
        cfg.defaults.notification_channel = settings.channel;
        cfg.defaults.telegram_chat_id = settings.telegram_chat_id.clone();
        crate::notify_channels::validate_module(&cfg.defaults)?;
        if let Some(credential) = credential {
            crate::notify_channels::store_credential(settings.channel, credential)?;
        }
        crate::config::save(&cfg, None)
    }

    fn send_test_notification(&mut self) -> Result<()> {
        let cfg = crate::config::load(None)?;
        crate::notify_channels::send_activity(
            &cfg,
            "Test notification",
            "Parasaur notifications reach you here.",
            None,
        )
        .1
    }
}

/// The saved config, or empty defaults on a host that has none yet.
fn load_or_default_config() -> Result<crate::config::GahConfig> {
    if crate::config::resolve_config_path(None).exists() {
        crate::config::load(None)
    } else {
        Ok(crate::config::GahConfig {
            defaults: Default::default(),
            profiles: Default::default(),
            context: Default::default(),
        })
    }
}

/// `gah setup` flags. Choices only: secrets come from the environment or a
/// hidden prompt, so they stay out of shell history and process lists.
#[derive(clap::Args)]
pub struct Args {
    /// What this machine is for: central runs the dashboard.
    #[arg(long, value_enum)]
    role: Option<requirements::Role>,
    /// Factory automation module (fresh standalone default: false; upgrades preserve state).
    #[arg(long, action = clap::ArgAction::Set)]
    factory_enabled: Option<bool>,
    /// Telegram/Discord notification module (default: off; upgrades with a
    /// channel stay on). `false` switches it off without asking anything.
    #[arg(long, action = clap::ArgAction::Set)]
    notifications_enabled: Option<bool>,
    /// Run only the notification step: pick Telegram or Discord, enter the
    /// credential (hidden), and send a test message.
    #[arg(long, conflicts_with = "check")]
    notifications_only: bool,
    #[arg(long, value_enum)]
    agent: Option<requirements::Agent>,
    #[arg(long, value_enum)]
    provider: Option<requirements::Provider>,
    /// Shared memory for chats and dispatched work (default: off).
    #[arg(long, value_enum)]
    memory: Option<requirements::MemoryMode>,
    /// Manager Chat permission prompts on this machine (default: ask; unattended runs keep the stored mode).
    #[arg(long, value_enum)]
    chat_permissions: Option<wizard::ChatPermissions>,
    /// A repository checkout to add as the first project.
    #[arg(long)]
    project: Option<PathBuf>,
    /// Central node address (worker role). The token comes from COORDINATOR_TOKEN.
    #[arg(long)]
    central_url: Option<String>,
    /// Memory gateway address (remote memory). The key comes from GAH_GATEWAY_API_KEY.
    #[arg(long)]
    gateway_url: Option<String>,
    /// MemoryCore checkout (colocated memory). Optional generation credentials come from GAH_GATEWAY_LLM_API_KEY.
    #[arg(long)]
    memorycore: Option<PathBuf>,
    /// The git-agent-harness checkout to build (default: the one this gah came from).
    #[arg(long)]
    source: Option<PathBuf>,
    /// Take every default and accept every offer, without prompting.
    #[arg(long)]
    yes: bool,
    /// Only report what this machine has and lacks; change nothing.
    #[arg(long)]
    check: bool,
    /// With --check: print the machine-readable report.
    #[arg(long, requires = "check")]
    json: bool,
}

/// `gah setup`: the guided install, or `--check` for a read-only report.
pub fn run(args: Args) -> Result<()> {
    let host = host::SystemHost;
    if args.check {
        let selection = requirements::Selection {
            role: args.role.unwrap_or(requirements::Role::Central),
            agent: args.agent.unwrap_or(requirements::Agent::Claude),
            provider: args.provider.unwrap_or(requirements::Provider::Github),
            memory: args.memory.unwrap_or(requirements::MemoryMode::Off),
        };
        let mut report = requirements::report(selection, &host);
        // A config that does not load must not fail this read-only check: the
        // requirements are still worth reporting, with the factory shown off.
        report.factory_enabled = crate::factory::enabled(None).unwrap_or(false);
        report.factory_ready = report.factory_enabled && report.ready;
        report.notifications_enabled = load_or_default_config()
            .is_ok_and(|cfg| crate::notify_channels::module_enabled(&cfg.defaults));
        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            for requirement in &report.requirements {
                let mark = if requirement.status.is_ok() {
                    "✓"
                } else if requirement.optional {
                    "·"
                } else {
                    "✗"
                };
                println!("{mark} {}: {}", requirement.label, requirement.why);
            }
            println!(
                "{}",
                if report.ready {
                    "Ready."
                } else {
                    "Not ready: run `gah setup` to provide what is missing."
                }
            );
        }
        return Ok(());
    }
    let options = wizard::Options {
        role: args.role,
        factory_enabled: args.factory_enabled,
        notifications_enabled: args.notifications_enabled,
        agent: args.agent,
        provider: args.provider,
        memory: args.memory,
        project: args.project,
        central_url: args.central_url,
        gateway_url: args.gateway_url,
        memorycore: args.memorycore,
        chat_permissions: args.chat_permissions,
        // The notification step builds nothing, so it needs no checkout.
        source: if args.notifications_only {
            PathBuf::new()
        } else {
            wizard::find_source(args.source)?
        },
        yes: args.yes,
    };
    if !options.yes && !std::io::stdin().is_terminal() {
        bail!("gah setup asks questions; run it in a terminal, or pass --yes with the choices as flags.");
    }
    let mut prompter = TerminalPrompter;
    let mut effects = SystemEffects;
    if args.notifications_only {
        wizard::Setup {
            host: &host,
            prompter: &mut prompter,
            effects: &mut effects,
            options,
        }
        .notifications();
        return Ok(());
    }
    wizard::Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options,
    }
    .run()
}

#[cfg(test)]
mod tests {
    use super::wizard::{ChatPermissions, Effects};
    use super::SystemEffects;

    #[test]
    fn chat_permissions_keep_other_manager_chat_settings() {
        if std::env::var_os("GAH_MANAGER_CHAT_SETTINGS_PATH").is_some() {
            return;
        }
        let source = tempfile::tempdir().unwrap();
        let path = source.path().join("config/manager-chat-settings.json");
        SystemEffects
            .set_chat_permissions(source.path(), ChatPermissions::Bypass)
            .unwrap();
        std::fs::write(
            &path,
            r#"{"defaultBackend":"codex","permissionMode":"bypass"}"#,
        )
        .unwrap();
        SystemEffects
            .set_chat_permissions(source.path(), ChatPermissions::Ask)
            .unwrap();
        let stored: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            stored,
            serde_json::json!({"defaultBackend": "codex", "permissionMode": "ask"})
        );
        std::fs::write(&path, "not json").unwrap();
        assert!(SystemEffects
            .set_chat_permissions(source.path(), ChatPermissions::Bypass)
            .is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not json");
    }
}
