use super::*;
use crate::setup::host::{Os, PackageManager};
use crate::setup::requirements::tests::FakeHost;
use std::collections::VecDeque;

#[derive(Default)]
struct Script {
    answers: VecDeque<String>,
    said: Vec<String>,
    asked: Vec<String>,
}

impl Prompter for Script {
    fn say(&mut self, line: &str) {
        self.said.push(line.to_string());
    }
    fn choose(&mut self, question: &str, _options: &[String], _default: usize) -> Result<usize> {
        self.asked.push(question.to_string());
        Ok(self
            .answers
            .pop_front()
            .context("unexpected question")?
            .parse()?)
    }
    fn confirm(&mut self, question: &str, _default: bool) -> Result<bool> {
        self.asked.push(question.to_string());
        Ok(self
            .answers
            .pop_front()
            .context("unexpected confirmation")?
            == "y")
    }
    fn text(&mut self, question: &str, _default: Option<&str>) -> Result<String> {
        self.asked.push(question.to_string());
        self.answers.pop_front().context("unexpected text")
    }
    fn secret(&mut self, question: &str) -> Result<String> {
        self.asked.push(question.to_string());
        self.answers.pop_front().context("unexpected secret")
    }
}

#[derive(Default)]
struct Recorder {
    commands: Vec<(String, Vec<(String, String)>)>,
    http: Vec<(String, String)>,
    status: Option<u16>,
    profiles: Vec<String>,
    fails: bool,
    chat_permissions: Vec<ChatPermissions>,
    notifications: NotificationSettings,
    saved_notifications: Vec<(NotificationSettings, Option<String>)>,
    /// `Some(reason)` makes the save fail; the real one validates the credential.
    notification_save_error: Option<String>,
    test_notification_error: Option<String>,
    test_notifications: usize,
}

impl Effects for Recorder {
    fn run(&mut self, command: &str, _cwd: Option<&Path>, env: &[(&str, String)]) -> bool {
        self.commands.push((
            command.to_string(),
            env.iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        ));
        !self.fails
    }
    fn http(
        &mut self,
        method: &str,
        url: &str,
        _bearer: Option<&str>,
        _body: Option<&str>,
    ) -> Option<u16> {
        self.http.push((method.to_string(), url.to_string()));
        self.status
    }
    fn profile_exists(&mut self, profile: &str) -> bool {
        self.profiles.iter().any(|p| p == profile)
    }
    fn add_profile(&mut self, args: InitArgs) -> Result<()> {
        self.profiles.push(args.profile);
        Ok(())
    }
    fn set_chat_permissions(&mut self, _source: &Path, mode: ChatPermissions) -> Result<()> {
        self.chat_permissions.push(mode);
        Ok(())
    }
    fn notification_settings(&mut self) -> NotificationSettings {
        self.notifications.clone()
    }
    fn save_notification_settings(
        &mut self,
        settings: &NotificationSettings,
        credential: Option<&str>,
    ) -> Result<()> {
        if let Some(error) = &self.notification_save_error {
            bail!("{error}");
        }
        self.saved_notifications
            .push((settings.clone(), credential.map(str::to_string)));
        self.notifications = settings.clone();
        Ok(())
    }
    fn send_test_notification(&mut self) -> Result<()> {
        self.test_notifications += 1;
        match &self.test_notification_error {
            Some(error) => bail!("{error}"),
            None => Ok(()),
        }
    }
}

mod notifications {
    use super::*;
    use crate::notify_channels::NotificationChannel;

    fn step(options: Options, answers: &[&str], effects: &mut Recorder) -> Script {
        let host = ready_host();
        let mut prompter = Script {
            answers: answers.iter().map(|a| a.to_string()).collect(),
            ..Default::default()
        };
        Setup {
            host: &host,
            prompter: &mut prompter,
            effects,
            options,
        }
        .notifications();
        prompter
    }

    #[test]
    fn unattended_setup_leaves_notifications_alone_and_asks_nothing() {
        let mut effects = Recorder::default();
        let prompter = step(
            Options {
                yes: true,
                ..Default::default()
            },
            &[],
            &mut effects,
        );
        assert!(prompter.asked.is_empty());
        assert!(effects.saved_notifications.is_empty());
    }

    #[test]
    fn choosing_off_never_asks_for_a_token_or_chat_id() {
        let mut effects = Recorder::default();
        let prompter = step(Options::default(), &["0"], &mut effects);
        assert_eq!(prompter.asked, ["Send notifications to a chat app?"]);
        assert!(
            effects.saved_notifications.is_empty(),
            "fresh install stays off"
        );
        assert_eq!(effects.test_notifications, 0);
    }

    #[test]
    fn telegram_takes_token_and_chat_id_then_sends_a_test_in_one_step() {
        let mut effects = Recorder::default();
        let prompter = step(
            Options::default(),
            &["1", " 123456:secret-token ", "4242"],
            &mut effects,
        );
        assert_eq!(prompter.asked.len(), 3, "{:?}", prompter.asked);
        assert!(prompter.asked[1].contains("hidden"));
        let (saved, credential) = &effects.saved_notifications[0];
        assert!(saved.enabled);
        assert_eq!(saved.channel, NotificationChannel::Telegram);
        assert_eq!(saved.telegram_chat_id.as_deref(), Some("4242"));
        assert_eq!(credential.as_deref(), Some("123456:secret-token"));
        assert_eq!(effects.test_notifications, 1);
        let said = prompter.said.join("\n");
        assert!(
            said.contains("@BotFather") && said.contains("@userinfobot"),
            "{said}"
        );
        assert!(said.contains("Sent a test message"));
        assert!(!said.contains("secret-token"), "the token is never echoed");
    }

    #[test]
    fn a_failed_test_or_save_is_reported_and_never_fails_setup() {
        let mut effects = Recorder {
            test_notification_error: Some("Telegram notification rejected with HTTP 401".into()),
            ..Default::default()
        };
        let prompter = step(Options::default(), &["1", "1:t", "7"], &mut effects);
        assert!(prompter
            .said
            .join("\n")
            .contains("test message failed: Telegram notification rejected with HTTP 401"));
        assert!(
            effects.notifications.enabled,
            "the module stays on to be repaired"
        );

        let mut effects = Recorder {
            notification_save_error: Some("That is not a Telegram bot token.".into()),
            ..Default::default()
        };
        let prompter = step(Options::default(), &["1", "nope", "7"], &mut effects);
        assert!(prompter
            .said
            .join("\n")
            .contains("Notifications were not changed: That is not a Telegram bot token."));
        assert_eq!(effects.test_notifications, 0);
    }

    #[test]
    fn rerunning_setup_keeps_a_saved_channel_and_its_credential() {
        let mut effects = Recorder {
            notifications: NotificationSettings {
                enabled: true,
                channel: NotificationChannel::Discord,
                telegram_chat_id: None,
                credential_saved: true,
            },
            ..Default::default()
        };
        let prompter = step(Options::default(), &["2", ""], &mut effects);
        assert!(prompter.asked[1].contains("empty keeps the saved one"));
        let (saved, credential) = &effects.saved_notifications[0];
        assert_eq!(saved.channel, NotificationChannel::Discord);
        assert_eq!(credential, &None);
    }

    #[test]
    fn notifications_enabled_false_switches_off_without_questions() {
        let mut effects = Recorder {
            notifications: NotificationSettings {
                enabled: true,
                channel: NotificationChannel::Telegram,
                telegram_chat_id: Some("1".into()),
                credential_saved: true,
            },
            ..Default::default()
        };
        let prompter = step(
            Options {
                notifications_enabled: Some(false),
                ..Default::default()
            },
            &[],
            &mut effects,
        );
        assert!(prompter.asked.is_empty());
        let (saved, _) = &effects.saved_notifications[0];
        assert!(!saved.enabled);
        assert_eq!(
            saved.channel,
            NotificationChannel::Telegram,
            "kept for later"
        );
    }
}

fn ready_host() -> FakeHost {
    FakeHost::new(Os::Linux, Some(PackageManager::Apt))
        .with("git --version", true, "git version 2.43.0")
        .with("cargo --version", true, "cargo 1.94.1")
        .with("node --version", true, "v22.4.0")
        .with("claude --version", true, "2.1.0")
        .with("claude auth status --json", true, r#"{"loggedIn":true}"#)
        .with("gh --version", true, "gh version 2.60.0")
        .with(
            "gh auth status",
            true,
            "✓ Logged in to github.com account octo",
        )
        .with("curl --version", true, "curl 8.5.0")
        .with("systemctl --version", true, "systemd 255")
        .with("id -un", true, "testuser")
        .with_path(crate::setup::requirements::SYSTEMD_RUNNING)
        .with_path(crate::setup::requirements::linger_path("testuser"))
        .with("tailscale version", true, "1.76.0")
}

#[test]
fn colocated_memory_preserves_backend_configuration_without_model_key_prompt() {
    let path = PathBuf::from("/memory");
    let host = ready_host()
        .with_path(path.join("src/gateway/server.ts"))
        .with_path(path.join("node_modules"))
        .with_env("GAH_GATEWAY_PROVIDER", "ollama")
        .with_env("GAH_GATEWAY_ENDPOINT", "http://127.0.0.1:11434/v1")
        .with_env("GAH_GATEWAY_LLM_MODEL", "llama3")
        .with_env("GAH_GATEWAY_EMBEDDING_MODEL", "nomic-embed-text")
        .with_env("GAH_GATEWAY_EMBEDDING_DIMENSIONS", "");
    let mut prompter = Script::default();
    let mut effects = Recorder::default();
    let mut setup = Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: Options {
            memory: Some(MemoryMode::Colocated),
            memorycore: Some(path),
            ..Default::default()
        },
    };
    let mut env = InstallEnv::default();
    assert_eq!(
        setup.memory_settings(&mut env).unwrap(),
        MemoryMode::Colocated
    );
    assert!(prompter.asked.is_empty());
    assert!(effects.commands.is_empty());
    assert!(env
        .0
        .contains(&("GAH_GATEWAY_MEMORYCORE_PATH", "/memory".into())));
    assert!(!env
        .0
        .iter()
        .any(|(key, _)| *key == "GAH_GATEWAY_LLM_API_KEY"));
}

/// Issue #1319: colocated memory needs no model provider and no key.
#[test]
fn colocated_memory_needs_no_provider_or_key() {
    let path = PathBuf::from("/memory");
    let host = ready_host()
        .with_path(path.join("src/gateway/server.ts"))
        .with_path(path.join("node_modules"));
    let model_settings = |env: &InstallEnv| {
        env.0
            .iter()
            .filter(|(key, _)| {
                key.starts_with("GAH_GATEWAY_")
                    && !matches!(*key, "GAH_GATEWAY_MODE" | "GAH_GATEWAY_MEMORYCORE_PATH")
            })
            .count()
    };
    let options = |yes: bool| Options {
        memory: Some(MemoryMode::Colocated),
        memorycore: Some(path.clone()),
        yes,
        ..Default::default()
    };
    let mut effects = Recorder::default();

    // Unattended, with nothing in the environment.
    let mut prompter = Script::default();
    let mut env = InstallEnv::default();
    Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: options(true),
    }
    .memory_settings(&mut env)
    .unwrap();
    assert!(prompter.asked.is_empty(), "--yes never prompts");
    assert_eq!(model_settings(&env), 0, "{:?}", env.0);

    // Interactive: the default answer is no provider, and nothing more is asked.
    let mut prompter = Script {
        answers: ["0".to_string()].into(),
        ..Default::default()
    };
    let mut env = InstallEnv::default();
    Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: options(false),
    }
    .memory_settings(&mut env)
    .unwrap();
    assert_eq!(prompter.asked.len(), 1, "{:?}", prompter.asked);
    assert_eq!(model_settings(&env), 0, "{:?}", env.0);

    // Interactive Ollama: endpoint, models and dimensions, but no secret.
    let mut prompter = Script {
        answers: ["1", "", "", "", ""].map(String::from).into(),
        ..Default::default()
    };
    let mut env = InstallEnv::default();
    Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: options(false),
    }
    .memory_settings(&mut env)
    .unwrap();
    assert!(env.0.contains(&("GAH_GATEWAY_PROVIDER", "ollama".into())));
    assert!(env
        .0
        .contains(&("GAH_GATEWAY_ENDPOINT", "http://127.0.0.1:11434/v1".into())));
    assert!(
        !prompter.asked.iter().any(|q| q.contains("key")),
        "{:?}",
        prompter.asked
    );
    assert!(!env.0.iter().any(|(key, _)| key.ends_with("_API_KEY")));

    // The offer itself no longer says a key is needed.
    let mut prompter = Script {
        answers: ["0".to_string()].into(),
        ..Default::default()
    };
    let mut env = InstallEnv::default();
    Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: Options::default(),
    }
    .memory_settings(&mut env)
    .unwrap();
    let offer = prompter.said.join("\n");
    assert!(offer.contains("model provider"), "{offer}");
    assert!(offer.contains("is optional"), "{offer}");
    assert!(!offer.contains("API key."), "{offer}");

    // An unknown provider stops here, before anything is built.
    let typo = ready_host()
        .with_path(path.join("src/gateway/server.ts"))
        .with_path(path.join("node_modules"))
        .with_env("GAH_GATEWAY_PROVIDER", "olama");
    let mut env = InstallEnv::default();
    let error = Setup {
        host: &typo,
        prompter: &mut Script::default(),
        effects: &mut effects,
        options: options(true),
    }
    .memory_settings(&mut env)
    .unwrap_err();
    assert!(error.to_string().contains("ollama or openai"), "{error}");

    // Unattended OpenAI with a key already stored: the installer decides.
    let rerun = ready_host()
        .with_path(path.join("src/gateway/server.ts"))
        .with_path(path.join("node_modules"))
        .with_env("GAH_GATEWAY_PROVIDER", "openai");
    let mut env = InstallEnv::default();
    Setup {
        host: &rerun,
        prompter: &mut Script::default(),
        effects: &mut effects,
        options: options(true),
    }
    .memory_settings(&mut env)
    .unwrap();
    assert!(env.0.contains(&("GAH_GATEWAY_PROVIDER", "openai".into())));
    assert!(!env.0.iter().any(|(key, _)| key.ends_with("_API_KEY")));
}

#[test]
fn colocated_memory_forwards_a_generation_key_from_the_environment() {
    let path = PathBuf::from("/memory");
    let host = ready_host()
        .with_path(path.join("src/gateway/server.ts"))
        .with_path(path.join("node_modules"))
        .with_env("GAH_GATEWAY_LLM_API_KEY", "generation-canary")
        .with_env("GAH_GATEWAY_PROVIDER", "openai")
        .with_env("GAH_GATEWAY_ENDPOINT", "https://api.openai.com/v1")
        .with_env("GAH_GATEWAY_LLM_MODEL", "gpt-4o")
        .with_env("GAH_GATEWAY_EMBEDDING_MODEL", "text-embedding-3-small")
        .with_env("GAH_GATEWAY_EMBEDDING_API_KEY", "embedding-canary")
        .with_env("GAH_GATEWAY_EMBEDDING_DIMENSIONS", "");
    let mut prompter = Script::default();
    let mut effects = Recorder::default();
    let mut setup = Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: Options {
            memory: Some(MemoryMode::Colocated),
            memorycore: Some(path),
            ..Default::default()
        },
    };
    let mut env = InstallEnv::default();
    setup.memory_settings(&mut env).unwrap();
    assert!(prompter.asked.is_empty());
    assert!(env
        .0
        .contains(&("GAH_GATEWAY_LLM_API_KEY", "generation-canary".into())));
}

#[test]
fn installer_effects_are_disclosed_before_a_declined_confirmation() {
    for os in [Os::Linux, Os::Macos] {
        for memory in [MemoryMode::Off, MemoryMode::Remote, MemoryMode::Colocated] {
            let host = FakeHost::new(os, None);
            let mut prompter = Script {
                answers: ["n".to_string()].into(),
                ..Default::default()
            };
            let mut effects = Recorder::default();
            let result = Setup {
                host: &host,
                prompter: &mut prompter,
                effects: &mut effects,
                options: Options::default(),
            }
            .install(
                &Selection {
                    role: Role::Central,
                    agent: Agent::Claude,
                    provider: Provider::Github,
                    memory,
                },
                InstallEnv(vec![
                    ("GAH_GATEWAY_MEMORYCORE_PATH", "/chosen/MemoryCore".into()),
                    ("GAH_GATEWAY_LLM_API_KEY", "secret-must-not-appear".into()),
                ]),
            );
            assert!(result.is_err());
            assert!(effects.commands.is_empty());
            assert_eq!(prompter.asked.len(), 1);
            let plan = prompter.said.join("\n");
            assert!(plan.contains("Persist the selected node role"));
            assert_eq!(
                plan.contains("store gateway credentials")
                    || plan.contains("Store gateway credentials"),
                memory != MemoryMode::Off
            );
            assert_eq!(
                plan.contains("Check remote gateway reachability"),
                memory == MemoryMode::Remote
            );
            assert_eq!(
                plan.contains("Seed /chosen/MemoryCore/tdai-gateway.local.yaml"),
                memory == MemoryMode::Colocated
            );
            let service = if os == Os::Linux {
                "Install and enable/start user tdai-memory-gateway.service"
            } else {
                "Install and start the memory-gateway LaunchAgent"
            };
            assert_eq!(plan.contains(service), memory == MemoryMode::Colocated);
            assert!(!plan.contains("secret-must-not-appear"));
        }
    }
}

#[test]
fn a_ready_central_machine_asks_three_questions_then_installs() {
    let host = ready_host();
    let mut prompter = Script {
        answers: ["1", "", "0", "0", "0", "n", "y"].map(String::from).into(),
        ..Default::default()
    };
    let mut effects = Recorder::default();
    Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: Options {
            provider: None,
            source: PathBuf::from("/src"),
            ..Default::default()
        },
    }
    .run()
    .unwrap();
    assert_eq!(effects.commands.len(), 1);
    let (command, env) = &effects.commands[0];
    assert_eq!(command, "scripts/install.sh");
    assert!(env.contains(&("GAH_NODE_ROLE".into(), "central".into())));
    assert_eq!(
        effects.chat_permissions,
        [ChatPermissions::Ask],
        "declining keeps prompts on"
    );
    assert!(
        prompter
            .said
            .iter()
            .any(|line| line.contains("Shared memory: gah setup --memory")),
        "skipped memory says how to add it"
    );
}

#[test]
fn accepting_the_default_role_sets_up_standalone_so_networking_is_opt_in() {
    let host = ready_host();
    let mut prompter = Script {
        answers: ["0", "", "0", "0", "0", "y", "y"].map(String::from).into(),
        ..Default::default()
    };
    let mut effects = Recorder::default();
    Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: Options {
            provider: None,
            source: PathBuf::from("/src"),
            ..Default::default()
        },
    }
    .run()
    .unwrap();
    assert_eq!(effects.commands[0].0, "scripts/install.sh");
    assert!(effects.commands[0]
        .1
        .contains(&("GAH_INSTALL_AGENT".into(), "claude".into())));
    assert!(!prompter
        .said
        .iter()
        .any(|line| line.contains("opencode/agents") || line.contains("gah-quota-refresh")));
    assert!(effects.commands[0]
        .1
        .contains(&("GAH_NODE_ROLE".into(), "standalone".into())));
    assert!(prompter
        .said
        .iter()
        .any(|line| line.contains("http://127.0.0.1:3773")));
    assert_eq!(effects.chat_permissions, [ChatPermissions::Bypass]);
    assert!(prompter
        .asked
        .iter()
        .any(|question| question.contains("Skip permission prompts")));
}

/// #1382: an unattended run keeps the stored mode unless the flag sets it.
#[test]
fn unattended_setup_changes_chat_permissions_only_when_asked() {
    for (flag, stored) in [
        (None, vec![]),
        (Some(ChatPermissions::Bypass), vec![ChatPermissions::Bypass]),
        (Some(ChatPermissions::Ask), vec![ChatPermissions::Ask]),
    ] {
        let host = ready_host();
        let mut prompter = Script {
            answers: [""].map(String::from).into(),
            ..Default::default()
        };
        let mut effects = Recorder::default();
        Setup {
            host: &host,
            prompter: &mut prompter,
            effects: &mut effects,
            options: Options {
                role: Some(Role::Standalone),
                agent: Some(Agent::Claude),
                provider: Some(Provider::Github),
                memory: Some(MemoryMode::Off),
                chat_permissions: flag,
                source: PathBuf::from("/src"),
                yes: true,
                ..Default::default()
            },
        }
        .run()
        .unwrap();
        assert_eq!(effects.chat_permissions, stored, "{flag:?}");
    }
}

#[test]
fn standalone_setup_installs_the_local_control_plane() {
    let host = ready_host();
    let mut prompter = Script {
        answers: [""].map(String::from).into(),
        ..Default::default()
    };
    let mut effects = Recorder::default();
    Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: Options {
            role: Some(Role::Standalone),
            agent: Some(Agent::Claude),
            provider: Some(Provider::Github),
            memory: Some(MemoryMode::Off),
            source: PathBuf::from("/src"),
            yes: true,
            ..Default::default()
        },
    }
    .run()
    .unwrap();
    assert_eq!(effects.commands[0].0, "scripts/install.sh");
    assert!(effects.commands[0]
        .1
        .contains(&("GAH_INSTALL_AGENT".into(), "claude".into())));
    assert!(!prompter
        .said
        .iter()
        .any(|line| line.contains("opencode/agents") || line.contains("gah-quota-refresh")));
    assert!(effects.commands[0]
        .1
        .contains(&("GAH_NODE_ROLE".into(), "standalone".into())));
    assert!(prompter
        .said
        .iter()
        .any(|line| line.contains("http://127.0.0.1:3773")));
}

#[test]
fn a_clionly_machine_installs_with_locked() {
    let host = ready_host();
    let mut prompter = Script {
        answers: ["", "y"].map(String::from).into(),
        ..Default::default()
    };
    let mut effects = Recorder::default();
    Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: Options {
            role: Some(Role::CliOnly),
            agent: Some(Agent::Claude),
            provider: Some(Provider::Github),
            source: PathBuf::from("/src"),
            ..Default::default()
        },
    }
    .run()
    .unwrap();
    assert_eq!(effects.commands.len(), 1);
    let (command, _env) = &effects.commands[0];
    assert_eq!(
        command,
        "cargo metadata --locked --format-version 1 >/dev/null && cargo install --path . --bin gah --force --locked"
    );
}

#[test]
fn a_missing_prerequisite_is_offered_and_a_declined_one_stops_before_building() {
    let host = FakeHost::new(Os::Linux, Some(PackageManager::Apt));
    let options = Options {
        role: Some(Role::CliOnly),
        agent: Some(Agent::Claude),
        provider: Some(Provider::Github),
        source: PathBuf::from("/src"),
        ..Default::default()
    };
    // Blank project path, then decline every offer.
    let mut prompter = Script {
        answers: std::iter::once(String::new())
            .chain(std::iter::repeat_n("n".to_string(), 10))
            .collect(),
        ..Default::default()
    };
    let mut effects = Recorder::default();
    let error = Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options,
    }
    .run()
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("run `/src/target/release/gah setup` again"),
        "before gah is installed, the hint names the binary the paste script built"
    );
    assert!(
        effects.commands.is_empty(),
        "nothing is installed or built without a yes"
    );
    assert!(prompter
        .asked
        .iter()
        .any(|q| q.contains("sudo apt-get install -y git") && q.contains("password")));
    assert!(prompter.said.iter().any(|line| line.contains("✗ git")));
    assert!(
        !prompter
            .asked
            .iter()
            .any(|q| q.contains("npm install") || q.contains("auth login")),
        "nothing is offered before what it needs: npm needs Node, a login needs its program"
    );
}

#[test]
fn a_worker_checks_its_central_node_before_building() {
    let host = ready_host();
    let mut prompter = Script {
        answers: ["2", "0", "0", "https://central.example/", "token-123", "y"]
            .map(String::from)
            .into(),
        ..Default::default()
    };
    let mut effects = Recorder {
        status: Some(200),
        ..Default::default()
    };
    Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: Options {
            source: PathBuf::from("/src"),
            ..Default::default()
        },
    }
    .run()
    .unwrap();
    assert_eq!(
        effects.http,
        [(
            "GET".to_string(),
            "https://central.example/api/info".to_string()
        )]
    );
    let env = &effects.commands[0].1;
    assert!(env.contains(&("GAH_NODE_ROLE".into(), "worker".into())));
    assert!(env.contains(&("GAH_CENTRAL_URL".into(), "https://central.example".into())));
    assert!(env.contains(&("COORDINATOR_TOKEN".into(), "token-123".into())));

    let mut prompter = Script {
        answers: ["2", "0", "0", "https://central.example", "bad"]
            .map(String::from)
            .into(),
        ..Default::default()
    };
    let mut effects = Recorder {
        status: Some(401),
        ..Default::default()
    };
    let error = Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: Options {
            source: PathBuf::from("/src"),
            ..Default::default()
        },
    }
    .run()
    .unwrap_err();
    assert!(error.to_string().contains("rejected that token"));
    assert!(effects.commands.is_empty());
}

#[test]
fn unattended_setup_needs_secrets_from_the_environment() {
    let host = ready_host();
    let mut prompter = Script::default();
    let mut effects = Recorder::default();
    let options = Options {
        role: Some(Role::Worker),
        central_url: Some("https://c.example".into()),
        yes: true,
        source: PathBuf::from("/src"),
        ..Default::default()
    };
    let error = Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options,
    }
    .run()
    .unwrap_err();
    assert!(error.to_string().contains("COORDINATOR_TOKEN is required"));
    assert!(prompter.asked.is_empty(), "--yes never prompts");
}

/// A CLI-only machine where everything is ready except, maybe, the gh
/// login: `gh_status` is the `gh auth status` probe, `None` if it hung.
fn cli_setup(gh_status: Option<(bool, &str)>, fails: bool) -> (Result<()>, Script, Recorder) {
    let mut host = FakeHost::new(Os::Linux, Some(PackageManager::Apt))
        .with("git --version", true, "git version 2.43.0")
        .with("cargo --version", true, "cargo 1.94.1")
        .with("node --version", true, "v22.4.0")
        .with("claude --version", true, "2.1.0")
        .with("claude auth status --json", true, r#"{"loggedIn":true}"#)
        .with("gh --version", true, "gh version 2.45.0")
        .with("curl --version", true, "curl 8.5.0");
    if let Some((success, stderr)) = gh_status {
        host = host.with_streams("gh auth status", success, "", stderr);
    }
    let mut prompter = Script {
        answers: ["", "y", "y"].map(String::from).into(),
        ..Default::default()
    };
    let mut effects = Recorder {
        fails,
        ..Default::default()
    };
    let result = Setup {
        host: &host,
        prompter: &mut prompter,
        effects: &mut effects,
        options: Options {
            role: Some(Role::CliOnly),
            agent: Some(Agent::Claude),
            provider: Some(Provider::Github),
            source: PathBuf::from("/src"),
            ..Default::default()
        },
    }
    .run();
    (result, prompter, effects)
}

/// Issue #1324: a failed `gh auth login` stops setup with the next step
/// instead of moving on to the next question.
#[test]
fn a_failed_login_stops_setup_and_says_what_to_do() {
    let (result, prompter, effects) = cli_setup(
        Some((
            false,
            "You are not logged into any GitHub hosts. To log in, run: gh auth login\n",
        )),
        true,
    );
    let error = result.unwrap_err().to_string();
    assert!(
        error.contains("Run `gh auth login` in this terminal"),
        "{error}"
    );
    assert_eq!(
        effects.commands.len(),
        1,
        "nothing runs after the failed login"
    );
    assert!(
        prompter.asked.last().unwrap().contains("gh auth login"),
        "no question follows the failed login: {:?}",
        prompter.asked
    );
}

/// Issue #1324: a status check that did not settle the login says
/// nothing about it, so setup never sends the user through `gh auth
/// login` and does not refuse to build; it warns and continues.
#[test]
fn an_unconfirmed_login_warns_and_continues() {
    for gh_status in [None, Some((true, "unrecognized output"))] {
        let (result, prompter, effects) = cli_setup(gh_status, false);
        result.unwrap();
        assert!(
            !prompter.asked.iter().any(|q| q.contains("auth login")),
            "{:?}",
            prompter.asked
        );
        assert_eq!(effects.commands.len(), 1, "the build still runs");
        assert!(prompter
            .said
            .iter()
            .any(|line| line.contains("The login may still be valid")));
    }
}
