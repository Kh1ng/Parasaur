use super::*;

#[test]
fn config_orchestration_startup_validation_and_redacted_show() {
    let tmp = test_tempdir();
    let path = tmp.path().join("config.toml");
    let gah = || {
        let mut command = bin();
        command
            .env("GAH_CONFIG", &path)
            .env_remove("GAH_SUPABASE_URL")
            .env_remove("GAH_SUPABASE_ANON_KEY")
            .env(
                "GAH_SUPABASE_SERVICE_ROLE_KEY",
                "obviously-fake-service-key",
            );
        command
    };
    // This is also the server's startup config-validation path.
    for config in ["", "[defaults]\norchestration_mode = \"local\"\n"] {
        fs::write(&path, config).unwrap();
        gah()
            .args(["status", "--role", "--json"])
            .assert()
            .success();
    }
    for mode in ["hosted", "self_hosted"] {
        fs::write(
            &path,
            format!("[defaults]\norchestration_mode = \"{mode}\"\n"),
        )
        .unwrap();
        gah()
            .args(["status", "--role", "--json"])
            .assert()
            .failure()
            .stderr(predicates::str::contains("GAH_SUPABASE_URL"));
        gah()
            .env("GAH_SUPABASE_URL", "https://supabase.example.invalid")
            .args(["status", "--role", "--json"])
            .assert()
            .failure()
            .stderr(predicates::str::contains("GAH_SUPABASE_ANON_KEY"));
        for json in [false, true] {
            let mut command = gah();
            command
                .env("GAH_SUPABASE_URL", "https://supabase.example.invalid")
                .env("GAH_SUPABASE_ANON_KEY", "obviously-fake-anon-key")
                .args(["config", "show"]);
            if json {
                command.args(["--json", "--full"]);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains(mode), "{stdout}");
            assert!(stdout.contains("[REDACTED]"), "{stdout}");
            for secret in [
                "https://supabase.example.invalid",
                "obviously-fake-anon-key",
                "obviously-fake-service-key",
                "GAH_SUPABASE_SERVICE_ROLE_KEY",
            ] {
                assert!(!stdout.contains(secret), "{stdout}");
                assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
            }
            if json {
                let payload: Value = serde_json::from_str(&stdout).unwrap();
                assert_eq!(payload["orchestration_mode"], mode);
                assert_eq!(payload["supabase"]["url"], "[REDACTED]");
                assert_eq!(payload["supabase"]["anon_key"], "[REDACTED]");
            }
        }
        gah()
            .env("GAH_SUPABASE_URL", "https://supabase.example.invalid")
            .env("GAH_SUPABASE_ANON_KEY", "obviously-fake-anon-key")
            .env_remove("GAH_SUPABASE_SERVICE_ROLE_KEY")
            .args(["status", "--role", "--json"])
            .assert()
            .success();
    }
}

pub(super) fn config_with_profile() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = test_tempdir();
    let config = tmp.path().join("config.toml");
    let artifact_root = tmp.path().join("artifacts");
    std::fs::write(&config, "[defaults]\ncurrent_manager = \"codex\"\n").unwrap();

    bin()
        .args([
            "profile",
            "add",
            "test",
            "--display-name",
            "Test",
            "--repo-id",
            "owner/repo",
            "--provider",
            "github",
            "--repo",
            "owner/repo",
            "--local-path",
            tmp.path().to_str().unwrap(),
            "--artifact-root",
            artifact_root.to_str().unwrap(),
            "--config-path",
            config.to_str().unwrap(),
        ])
        .assert()
        .success();

    (tmp, config)
}

#[test]
fn bare_json_shape_remains_byte_for_byte_compatible() {
    let (_tmp, config) = config_with_profile();

    bin()
        .args([
            "config",
            "show",
            "--json",
            "--config",
            config.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout("{\"current_manager\":\"codex\"}\n");
}

#[test]
fn full_json_is_versioned_and_profile_keyed() {
    let (_tmp, config) = config_with_profile();
    let output = bin()
        .args([
            "config",
            "show",
            "--json",
            "--full",
            "--profile",
            "test",
            "--config",
            config.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["schema_version"], 4);
    assert_eq!(payload["config_path"], config.to_string_lossy().as_ref());
    assert_eq!(payload["profiles"]["test"]["profile"], "test");
    assert_eq!(payload["profiles"]["test"]["delivery_mode"], "pr");
    assert_eq!(payload["profiles"].as_object().unwrap().len(), 1);
}

#[test]
fn full_and_profile_require_machine_readable_mode() {
    bin().args(["config", "show", "--full"]).assert().failure();
    bin()
        .args(["config", "show", "--json", "--profile", "test"])
        .assert()
        .failure();
}

#[test]
fn node_role_and_central_url_survive_config_changes_and_need_no_profile() {
    let tmp = test_tempdir();
    let path = tmp.path().join("config.toml");
    let config = path.to_str().unwrap();
    bin()
        .args([
            "config",
            "set",
            "--config",
            config,
            "--node-role",
            "worker",
            "--registry-central-url",
            "http://192.168.1.10:3773",
        ])
        .assert()
        .success();
    bin()
        .args([
            "config",
            "set",
            "--config",
            config,
            "--current-manager",
            "claude",
        ])
        .assert()
        .success();
    let output = bin()
        .args(["status", "--role", "--json", "--config-path", config])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(node["role"], "worker");
    let overridden = bin()
        .env("GAH_NODE_ROLE", "central")
        .args(["status", "--role", "--json", "--config-path", config])
        .output()
        .unwrap();
    assert!(overridden.status.success());
    let overridden: serde_json::Value = serde_json::from_slice(&overridden.stdout).unwrap();
    assert_eq!(overridden["role"], "central");
    bin()
        .env("GAH_NODE_ROLE", "typo")
        .args(["status", "--role", "--json", "--config-path", config])
        .assert()
        .failure();
    assert_eq!(node["central_url"], "http://192.168.1.10:3773");
    let before = std::fs::read(&path).unwrap();
    bin()
        .args([
            "config",
            "set",
            "--config",
            config,
            "--clear",
            "registry_central_url",
        ])
        .assert()
        .failure();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    bin()
        .args([
            "config",
            "set",
            "--config",
            config,
            "--node-role",
            "central",
        ])
        .assert()
        .success();
    let output = bin()
        .args(["status", "--role", "--json", "--config-path", config])
        .output()
        .unwrap();
    let node: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(node["role"], "central");
}

#[test]
fn set_backend_instance_enabled_writes_merged_entry_and_flips_state() {
    let (tmp, config) = config_with_profile();
    let config_path = config.to_str().unwrap();
    // Declare the instance at the repo-defaults level; the profile-level
    // toggle must preserve every declared field (entries replace wholesale).
    // Append the shared-instance declaration as its own table (distinct
    // table names keep TOML order-independent).
    std::fs::write(
        &config,
        format!(
            "{}\n[defaults.routing.backend_instances.codex-paid]\n\
             runner_kind = \"codex\"\n\
             logical_backend = \"codex\"\n\
             executable = \"{}\"\n\
             account_label = \"team-api\"\n\
             supported_models = [\"openai/gpt-5\"]\n",
            std::fs::read_to_string(&config).unwrap(),
            std::env::current_exe().unwrap().display()
        ),
    )
    .unwrap();

    // Unknown instance is rejected.
    bin()
        .args([
            "config",
            "set-backend-instance-enabled",
            "--config",
            config_path,
            "--profile",
            "test",
            "--instance",
            "does-not-exist",
            "--enabled",
            "false",
        ])
        .assert()
        .failure();

    bin()
        .args([
            "config",
            "set-backend-instance-enabled",
            "--config",
            config_path,
            "--profile",
            "test",
            "--instance",
            "codex-paid",
            "--enabled",
            "false",
        ])
        .assert()
        .success();

    let saved = std::fs::read_to_string(&config).unwrap();
    // The profile-level override entry (not the defaults one) carries the
    // flipped flag, and the merged entry preserves every declared field.
    assert!(
        saved.contains("[profiles.test.routing.backend_instances.codex-paid]"),
        "profile-level override entry must be written, got: {saved}"
    );
    let profile_section = saved
        .split("[profiles.test.routing.backend_instances.codex-paid]")
        .nth(1)
        .expect("profile-level entry written");
    assert!(
        profile_section.contains("enabled = false"),
        "flag must be flipped, got: {saved}"
    );
    assert!(
        profile_section.contains(&format!(
            "executable = \"{}\"",
            std::env::current_exe().unwrap().display()
        )),
        "merged entry must preserve declared fields, got: {saved}"
    );

    // Already in the target state: idempotent success.
    bin()
        .args([
            "config",
            "set-backend-instance-enabled",
            "--config",
            config_path,
            "--profile",
            "test",
            "--instance",
            "codex-paid",
            "--enabled",
            "false",
        ])
        .assert()
        .success();

    // Flip back on.
    bin()
        .args([
            "config",
            "set-backend-instance-enabled",
            "--config",
            config_path,
            "--profile",
            "test",
            "--instance",
            "codex-paid",
            "--enabled",
            "true",
        ])
        .assert()
        .success();
    let saved = std::fs::read_to_string(&config).unwrap();
    let profile_section = saved
        .split("[profiles.test.routing.backend_instances.codex-paid]")
        .nth(1)
        .expect("profile-level entry still present");
    assert!(
        profile_section.contains("enabled = true"),
        "flag must flip back, got: {saved}"
    );

    // Unknown profile is rejected.
    bin()
        .args([
            "config",
            "set-backend-instance-enabled",
            "--config",
            config_path,
            "--profile",
            "missing",
            "--instance",
            "codex-paid",
            "--enabled",
            "false",
        ])
        .assert()
        .failure();

    let _ = tmp;
}

#[test]
fn add_backend_instance_creates_isolated_runtime_and_safe_projection() {
    let (tmp, config) = config_with_profile();
    let fake_bin = tmp.path().join("bin");
    fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        "#!/bin/sh\nif [ \"$1\" = \"login\" ]; then echo 'Logged in using ChatGPT'; echo 'token=super-secret' >&2; fi\n",
    );
    let path = prepend_path(&fake_bin);
    let home = tmp.path().join("home");
    bin()
        .env("HOME", &home)
        .env("PATH", &path)
        .args([
            "config",
            "add-backend-instance",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
            "--instance",
            "codex-work",
            "--runner-kind",
            "codex",
            "--account-label",
            "Work",
        ])
        .assert()
        .success();

    let output = bin()
        .env("HOME", &home)
        .env("PATH", &path)
        .args([
            "config",
            "show-backend-instance-runtime",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
            "--instance",
            "codex-work",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let runtime: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(runtime["backend_instance"], "codex-work");
    assert_eq!(runtime["account_label"], "Work");
    assert_eq!(
        runtime["state_root"],
        home.join(".config/gah/backend-instances/codex-work")
            .to_string_lossy()
            .as_ref()
    );

    let projected = bin()
        .env("HOME", &home)
        .env("PATH", &path)
        .args([
            "config",
            "test-backend-instance",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
            "--instance",
            "codex-work",
        ])
        .output()
        .unwrap();
    assert!(projected.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&projected.stdout).unwrap()["auth_ready"],
        true
    );
    assert!(!String::from_utf8_lossy(&projected.stdout).contains("super-secret"));
    assert!(!String::from_utf8_lossy(&projected.stderr).contains("super-secret"));
}

#[test]
fn notification_channel_settings_round_trip_and_validate() {
    let (tmp, config) = config_with_profile();
    let config_path = config.to_str().unwrap();

    // Invalid channel is rejected with the expected vocabulary.
    bin()
        .args([
            "config",
            "set",
            "--config",
            config_path,
            "--notification-channel",
            "slack",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("expected none|telegram|discord"));

    bin()
        .args([
            "config",
            "set",
            "--config",
            config_path,
            "--notification-channel",
            "telegram",
            "--telegram-chat-id",
            "123456789",
        ])
        .assert()
        .success();

    let saved = std::fs::read_to_string(&config).unwrap();
    assert!(
        saved.contains("notification_channel = \"telegram\""),
        "got: {saved}"
    );
    assert!(
        saved.contains("telegram_chat_id = \"123456789\""),
        "got: {saved}"
    );

    // Clearing the chat id works; the channel persists.
    bin()
        .args([
            "config",
            "set",
            "--config",
            config_path,
            "--telegram-chat-id",
            "",
        ])
        .assert()
        .success();
    let saved = std::fs::read_to_string(&config).unwrap();
    assert!(
        saved.contains("notification_channel = \"telegram\""),
        "got: {saved}"
    );
    assert!(!saved.contains("telegram_chat_id ="), "got: {saved}");

    let _ = tmp;
}

/// Issue #1454: the notification module switch end to end through the CLI.
#[test]
fn notification_module_switch_credential_and_notify_send() {
    let (tmp, config) = config_with_profile();
    let config_path = config.to_str().unwrap();
    let credentials = tmp.path().join("notify.env");
    let gah = || {
        let mut command = bin();
        command
            .env("GAH_NOTIFY_CREDENTIALS_FILE", &credentials)
            .env_remove("TELEGRAM_BOT_TOKEN")
            .env_remove("DISCORD_WEBHOOK_URL");
        command
    };
    let show = || -> Value {
        let output = gah()
            .args([
                "config",
                "show",
                "--json",
                "--full",
                "--config",
                config_path,
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["notifications"].clone()
    };
    let notify = || {
        gah()
            .args([
                "notify-send",
                "--config-path",
                config_path,
                "--title",
                "t",
                "--message",
                "m",
            ])
            .output()
            .unwrap()
    };

    // Fresh config: off, and notify-send is a silent success.
    assert_eq!(show()["enabled"], false);
    let sent = notify();
    assert!(sent.status.success());
    assert_eq!(
        String::from_utf8_lossy(&sent.stdout).trim(),
        r#"{"channel":"none","ok":true}"#
    );
    assert!(
        sent.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&sent.stderr)
    );

    // On needs a channel (fail closed).
    gah()
        .args([
            "config",
            "set",
            "--config",
            config_path,
            "--notifications-enabled",
            "true",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Notifications need a channel"));

    // On with a channel but no credential: a clear, non-blocking failure line.
    gah()
        .args([
            "config",
            "set",
            "--config",
            config_path,
            "--notifications-enabled",
            "true",
            "--notification-channel",
            "telegram",
            "--telegram-chat-id",
            "42",
        ])
        .assert()
        .success();
    let failed = notify();
    assert!(!failed.status.success());
    let receipt: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(receipt["channel"], "telegram");
    assert_eq!(receipt["ok"], false);
    assert!(receipt["reason"]
        .as_str()
        .unwrap()
        .contains("TELEGRAM_BOT_TOKEN is not set"));
    assert!(show()["credential_source"].is_null());

    // The credential arrives on stdin, never argv, and lands in a 0600 file.
    gah()
        .args([
            "config",
            "set",
            "--config",
            config_path,
            "--notification-credential-stdin",
        ])
        .write_stdin("not-a-token")
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a Telegram bot token"));
    gah()
        .args([
            "config",
            "set",
            "--config",
            config_path,
            "--notification-credential-stdin",
        ])
        .write_stdin("123456:cli-secret\n")
        .assert()
        .success();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&credentials)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let shown = show();
    assert_eq!(shown["credential_source"], "file");
    assert!(!shown.to_string().contains("cli-secret"));
    assert!(!std::fs::read_to_string(&config)
        .unwrap()
        .contains("cli-secret"));

    // Off again: silent, and the channel is remembered for later.
    gah()
        .args([
            "config",
            "set",
            "--config",
            config_path,
            "--notifications-enabled",
            "false",
        ])
        .assert()
        .success();
    let sent = notify();
    assert!(sent.status.success());
    assert!(sent.stderr.is_empty());
    let shown = show();
    assert_eq!(shown["enabled"], false);
    assert_eq!(shown["channel"], "telegram");
}

#[test]
fn routing_candidate_add_remove_move_round_trip_with_preview_and_validation() {
    let (tmp, config) = config_with_profile();
    let config_path = config.to_str().unwrap();

    // Add two candidates (one paid, approval-gated).
    bin()
        .args([
            "config",
            "routing-candidate",
            "add",
            "--config",
            config_path,
            "--profile",
            "test",
            "--list",
            "improve",
            "--backend",
            "codex",
            "--model",
            "gpt-5.4-mini",
        ])
        .assert()
        .success();
    bin()
        .args([
            "config",
            "routing-candidate",
            "add",
            "--config",
            config_path,
            "--profile",
            "test",
            "--list",
            "improve",
            "--backend",
            "vibe",
            "--requires-approval",
        ])
        .assert()
        .success();

    let output = bin()
        .args([
            "config",
            "routing-candidate",
            "add",
            "--config",
            config_path,
            "--profile",
            "test",
            "--list",
            "improve",
            "--backend",
            "claude",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let preview: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        preview.as_array().unwrap().len(),
        3,
        "dry-run previews three entries"
    );
    // Dry-run must not have saved.
    let saved = std::fs::read_to_string(&config).unwrap();
    assert!(
        !saved.contains("\"claude\""),
        "dry-run must not persist, got: {saved}"
    );

    // Reorder: move claude's slot (would-be index 2 doesn't exist; move 1 -> 0).
    bin()
        .args([
            "config",
            "routing-candidate",
            "move",
            "--config",
            config_path,
            "--profile",
            "test",
            "--list",
            "improve",
            "--from",
            "1",
            "--to",
            "0",
        ])
        .assert()
        .success();
    let saved = std::fs::read_to_string(&config).unwrap();
    let profile_section = saved
        .split("[profiles.test.routing.improve_candidates]")
        .nth(1)
        .map(|section| section.split("\n\n").next().unwrap_or(section));
    assert!(profile_section.is_some(), "profile-level list written");

    // Remove the first entry.
    bin()
        .args([
            "config",
            "routing-candidate",
            "remove",
            "--config",
            config_path,
            "--profile",
            "test",
            "--list",
            "improve",
            "--index",
            "0",
        ])
        .assert()
        .success();

    // Out-of-range index fails with a descriptive error.
    bin()
        .args([
            "config",
            "routing-candidate",
            "remove",
            "--config",
            config_path,
            "--profile",
            "test",
            "--list",
            "improve",
            "--index",
            "9",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("out of range"));

    // Invalid list name fails with the expected vocabulary.
    bin()
        .args([
            "config",
            "routing-candidate",
            "add",
            "--config",
            config_path,
            "--profile",
            "test",
            "--list",
            "squads",
            "--backend",
            "codex",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "expected pm|improve|review|escalatory",
        ));

    // Unknown profile fails.
    bin()
        .args([
            "config",
            "routing-candidate",
            "add",
            "--config",
            config_path,
            "--profile",
            "missing",
            "--list",
            "pm",
            "--backend",
            "codex",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not configured"));

    let _ = tmp;
}

#[test]
fn prompt_policy_cli_versions_audits_and_hides_content_from_show() {
    let (tmp, config) = config_with_profile();
    let path = config.to_str().unwrap();
    let events = tmp.path().join("events.jsonl");
    let content = "## Safety\nKeep the canary POLICY-CANARY-182 visible only in prompts.";

    bin()
        .env("GAH_EVENTS_PATH", &events)
        .args([
            "config",
            "prompt-policy",
            "set",
            "--profile",
            "test",
            "--slot",
            "worker_guidance",
            "--content",
            content,
            "--expected-revision",
            "0",
            "--config",
            path,
        ])
        .assert()
        .success();
    let audit = fs::read_to_string(&events).unwrap();
    assert!(audit.contains("prompt_policy_changed"));
    assert!(!audit.contains("POLICY-CANARY-182"));

    let shown = bin()
        .args([
            "config",
            "prompt-policy",
            "show",
            "--profile",
            "test",
            "--json",
            "--config",
            path,
        ])
        .output()
        .unwrap();
    assert!(shown.status.success());
    let summary: Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(summary["revision"], 1);
    assert!(summary["policies"].as_array().unwrap().iter().any(|entry| {
        entry["source"] == "profile_override"
            && entry["byte_size"] == content.len()
            && entry["sha256"].as_str().unwrap().starts_with("sha256:")
    }));
    assert!(!String::from_utf8_lossy(&shown.stdout).contains("POLICY-CANARY-182"));
}

#[test]
fn routing_candidate_routine_list_holds_one_reviewer() {
    let (tmp, config) = config_with_profile();
    let config_path = config.to_str().unwrap();
    let edit = |args: &[&str]| {
        let mut full = vec![
            "config",
            "routing-candidate",
            args[0],
            "--config",
            config_path,
            "--profile",
            "test",
            "--list",
            "routine",
        ];
        full.extend_from_slice(&args[1..]);
        bin().args(full).assert().success();
        std::fs::read_to_string(&config).unwrap()
    };

    let saved = edit(&["add", "--backend", "opencode", "--model", "tak/glm"]);
    assert!(saved.contains("routing.routine_reviewer]"), "got: {saved}");
    assert!(saved.contains("tak/glm"), "got: {saved}");

    // A second add replaces the reviewer instead of appending.
    let saved = edit(&["add", "--backend", "codex"]);
    assert!(!saved.contains("tak/glm"), "got: {saved}");
    assert!(saved.contains("routing.routine_reviewer]"), "got: {saved}");

    let saved = edit(&["remove", "--index", "0"]);
    assert!(!saved.contains("routing.routine_reviewer]"), "got: {saved}");

    let _ = tmp;
}

#[test]
fn show_vibe_models_and_set_vibe_model_read_and_write_vibes_own_config() {
    let (tmp, config) = config_with_profile();
    let home = tmp.path().join("home");
    let vibe_config = home.join(".vibe/config.toml");
    std::fs::create_dir_all(vibe_config.parent().unwrap()).unwrap();
    std::fs::write(
        &vibe_config,
        "theme = \"auto\"\nactive_model = \"mistral-medium-3.5\"\n[[models]]\nname = \"mistral-vibe-cli-latest\"\nalias = \"mistral-medium-3.5\"\ndisplay_name = \"Mistral Medium 3.5\"\n[[models]]\nname = \"devstral\"\nalias = \"local\"\n",
    )
    .unwrap();
    // A dev shell exporting VIBE_HOME would redirect the ambient read, so
    // every child here strips it and pins HOME.
    let gah = || {
        let mut command = bin();
        command.env("HOME", &home);
        command.env_unset("VIBE_HOME");
        command
    };

    let show = |instance: Option<&str>| {
        let mut command = gah();
        command.args([
            "config",
            "show-vibe-models",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
        ]);
        if let Some(instance) = instance {
            command.args(["--instance", instance]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };

    let summary = show(None);
    let models: Vec<&str> = summary["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["id"].as_str().unwrap())
        .collect();
    assert_eq!(models, vec!["mistral-medium-3.5", "local"]);
    assert_eq!(summary["active_model"], "mistral-medium-3.5");

    gah()
        .args([
            "config",
            "set-vibe-model",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
            "--model",
            "local",
        ])
        .assert()
        .success();
    let saved = std::fs::read_to_string(&vibe_config).unwrap();
    assert_eq!(saved.matches("active_model").count(), 1);
    assert!(saved.contains("active_model = \"local\""), "got: {saved}");
    assert!(saved.contains("theme = \"auto\""), "got: {saved}");
    assert_eq!(show(None)["active_model"], "local");

    // An unconfigured model is refused without touching the file.
    gah()
        .args([
            "config",
            "set-vibe-model",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
            "--model",
            "not-configured",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not configured"));
    assert_eq!(std::fs::read_to_string(&vibe_config).unwrap(), saved);

    let _ = tmp;
}

#[test]
fn vibe_model_commands_scope_to_a_declared_backend_instance() {
    let (tmp, config) = config_with_profile();
    let home = tmp.path().join("home");
    let fake_bin = tmp.path().join("bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin(&fake_bin, "vibe");
    let path = prepend_path(&fake_bin);
    let gah = || {
        let mut command = bin();
        command.env("HOME", &home);
        command.env("PATH", &path);
        command.env_unset("VIBE_HOME");
        command
    };

    gah()
        .args([
            "config",
            "add-backend-instance",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
            "--instance",
            "vibe-work",
            "--runner-kind",
            "vibe",
            "--account-label",
            "Work",
        ])
        .assert()
        .success();

    // The instance's isolated config lists a different model than the
    // ambient installation, so the scoping assertions cannot pass by
    // reading the wrong file.
    let instance_config = home.join(".config/gah/backend-instances/vibe-work/.vibe/config.toml");
    std::fs::create_dir_all(instance_config.parent().unwrap()).unwrap();
    std::fs::write(
        &instance_config,
        "active_model = \"glm-5.3\"\n[[models]]\nname = \"glm-5.3\"\nalias = \"glm-5.3\"\n",
    )
    .unwrap();
    let ambient_config = home.join(".vibe/config.toml");
    std::fs::create_dir_all(ambient_config.parent().unwrap()).unwrap();
    std::fs::write(
        &ambient_config,
        "active_model = \"local\"\n[[models]]\nname = \"devstral\"\nalias = \"local\"\n",
    )
    .unwrap();

    let output = gah()
        .args([
            "config",
            "show-vibe-models",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
            "--instance",
            "vibe-work",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["models"][0]["id"], "glm-5.3");
    assert_eq!(summary["active_model"], "glm-5.3");

    gah()
        .args([
            "config",
            "set-vibe-model",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
            "--instance",
            "vibe-work",
            "--model",
            "glm-5.3",
        ])
        .assert()
        .success();
    assert!(std::fs::read_to_string(&instance_config)
        .unwrap()
        .contains("active_model = \"glm-5.3\""));
    assert!(
        !std::fs::read_to_string(&ambient_config)
            .unwrap()
            .contains("glm-5.3"),
        "the ambient installation is untouched"
    );

    // An instance of another runner kind never answers for Vibe.
    let codex_bin = tmp.path().join("codex-bin");
    std::fs::create_dir_all(&codex_bin).unwrap();
    make_fake_bin(&codex_bin, "codex");
    let codex_path = prepend_path(&codex_bin);
    let codex_gah = || {
        let mut command = bin();
        command.env("HOME", &home).env("PATH", &codex_path);
        command
    };
    codex_gah()
        .args([
            "config",
            "add-backend-instance",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
            "--instance",
            "codex-work",
            "--runner-kind",
            "codex",
            "--account-label",
            "Work",
        ])
        .assert()
        .success();
    codex_gah()
        .args([
            "config",
            "show-vibe-models",
            "--config",
            config.to_str().unwrap(),
            "--profile",
            "test",
            "--instance",
            "codex-work",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("not a Vibe instance"));

    let _ = tmp;
}
