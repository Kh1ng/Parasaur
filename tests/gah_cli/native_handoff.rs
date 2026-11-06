use super::*;

#[test]
fn native_work_check_reobserves_controller_eligibility_without_backend_or_provider_mutation() {
    let tmp = test_tempdir();
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo);
    let config = write_real_repo_config(&tmp, &repo, "github");
    let fake_bin = tmp.path().join("bin");
    let forge = support::forge::FakeForge::new(&tmp.path().join("forge"), &fake_bin);
    forge.update(|state| {
        state["repo"] = serde_json::json!("owner/real");
        state["login"] = serde_json::json!("owner");
    });
    let seeded = forge
        .command("gh")
        .args([
            "api",
            "repos/owner/real/issues",
            "--method",
            "POST",
            "--raw-field",
            "title=Native eligibility fixture",
            "--raw-field",
            "body=Acceptance criteria: verify without running a backend",
            "--raw-field",
            "labels=exec:autonomous",
        ])
        .output()
        .unwrap();
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let canary = tmp.path().join("backend-invoked");
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!("#!/bin/sh\ntouch '{}'\nexit 91\n", canary.display()),
    );
    let request = serde_json::json!({"args":{"profile":"real","mode":"fix","backend":"codex","model":"fixture-model","target":"#1",
        "current_branch":false,"dry_run":false,"retries":2,"allow_draft_fail":false,"prod":false,"issue_intake_override":false,
        "allow_unknown_red_baseline":false,"escalate":false,"skip_validation_gate":false,"work_id":"#1","run_id":uuid::Uuid::new_v4().to_string(),"dispatch_reason":"initial"}});
    let request_file = tmp.path().join("request.json");
    fs::write(&request_file, serde_json::to_vec(&request).unwrap()).unwrap();
    let input = tmp.path().join("context");
    let ledger = tmp.path().join("history.jsonl");
    let prepared = bin()
        .args([
            "native-handoff",
            "prepare",
            "--request-file",
            request_file.to_str().unwrap(),
            "--output",
            input.to_str().unwrap(),
            "--node-id",
            "fixture-worker",
            "--central-url",
            "https://central.invalid",
            "--config-path",
            config.to_str().unwrap(),
        ])
        .env("GAH_LEDGER_PATH", &ledger)
        .assert()
        .success();
    let bound: Value = serde_json::from_slice(&prepared.get_output().stdout).unwrap();
    let check = |flag: &str| {
        let mut command = bin();
        command
            .args([
                "native-handoff",
                "check-current-policy",
                flag,
                "--context-dir",
                input.to_str().unwrap(),
                "--expected-context",
                bound["context_sha256"].as_str().unwrap(),
                "--node-id",
                "fixture-worker",
                "--central-url",
                "https://central.invalid",
                "--config-path",
                config.to_str().unwrap(),
            ])
            .env("PATH", prepend_path(&fake_bin))
            .env("GITHUB_TOKEN", "fixture-token")
            .env("GAH_LEDGER_PATH", &ledger);
        command
    };
    let calls_before = forge.calls().len();
    let ready = check("--check-work").assert().success();
    let ready: Value = serde_json::from_slice(&ready.get_output().stdout).unwrap();
    assert_eq!(ready["work_current"], true);
    assert_eq!(ready["execution_authorized"], false);
    let continued = check("--check-route").assert().success();
    let continued: Value = serde_json::from_slice(&continued.get_output().stdout).unwrap();
    assert_eq!(continued["route_current"], true);
    assert!(continued["work_current"].is_null());
    assert_eq!(continued["execution_authorized"], false);
    forge.update(|state| {
        state["issues"]["1"]["labels"] = serde_json::json!(["exec:human-required"])
    });
    check("--check-work")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "no longer the current controller action",
        ));
    assert!(
        !canary.exists(),
        "A work check must never start the selected backend"
    );
    for call in forge.calls().iter().skip(calls_before) {
        assert!(
            !call
                .iter()
                .any(|arg| matches!(arg.as_str(), "POST" | "PUT" | "PATCH" | "DELETE")),
            "A work check must not mutate forge state: {call:?}"
        );
    }
}

#[test]
fn native_context_cli_moves_history_and_verifies_the_expected_node_without_starting_a_backend() {
    let tmp = test_tempdir();
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let config_path = write_real_repo_config(&tmp, &repo, "github");
    let mut central: git_agent_harness::config::GahConfig =
        toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    central
        .profiles
        .get_mut("real")
        .unwrap()
        .routing
        .max_paid_reviews_per_ticket = Some(1);
    fs::write(&config_path, toml::to_string(&central).unwrap()).unwrap();
    let history = tmp.path().join("central-history.jsonl");
    let mut entry = git_agent_harness::ledger::LedgerEntry::new(
        "real",
        &central.profiles["real"],
        "codex",
        "fix",
        "old work",
        Some(uuid::Uuid::new_v4().to_string()),
        None,
    );
    entry.work_id = Some("#1".into());
    fs::write(
        &history,
        format!("{}\n", serde_json::to_string(&entry).unwrap()),
    )
    .unwrap();
    let request = serde_json::json!({"args": {"profile":"real", "mode":"fix", "backend":"auto", "target":repo.join("docs/task.md"),
        "current_branch":false, "dry_run":false, "retries":2, "allow_draft_fail":false, "prod":false,
        "issue_intake_override":false, "allow_unknown_red_baseline":false, "escalate":false, "skip_validation_gate":false,
        "work_id":"#1", "run_id":uuid::Uuid::new_v4().to_string(), "dispatch_reason":"initial"}});
    let request_file = tmp.path().join("request.json");
    fs::write(&request_file, serde_json::to_vec(&request).unwrap()).unwrap();
    let input = tmp.path().join("prepared");
    let prepared = bin()
        .args([
            "native-handoff",
            "prepare",
            "--request-file",
            request_file.to_str().unwrap(),
            "--output",
            input.to_str().unwrap(),
            "--node-id",
            "worker-native-test",
            "--central-url",
            "https://central.invalid",
            "--config-path",
            config_path.to_str().unwrap(),
        ])
        .env("GAH_LEDGER_PATH", &history)
        .assert()
        .success();
    let binding: Value = serde_json::from_slice(&prepared.get_output().stdout).unwrap();
    let check_current = || {
        let mut command = bin();
        command
            .args([
                "native-handoff",
                "check-current-policy",
                "--context-dir",
                input.to_str().unwrap(),
                "--expected-context",
                binding["context_sha256"].as_str().unwrap(),
                "--node-id",
                "worker-native-test",
                "--central-url",
                "https://central.invalid",
                "--config-path",
                config_path.to_str().unwrap(),
            ])
            .env("GAH_LEDGER_PATH", &history);
        command
    };
    let current = check_current().assert().success();
    let current: Value = serde_json::from_slice(&current.get_output().stdout).unwrap();
    assert_eq!(current["binding"], binding);
    assert_eq!(current["policy_current"], true);
    assert_eq!(current["execution_authorized"], false);
    let mut paused = central.clone();
    paused.defaults.factory_enabled = Some(false);
    fs::write(&config_path, toml::to_string(&paused).unwrap()).unwrap();
    check_current()
        .assert()
        .failure()
        .stderr(predicates::str::contains("disabled centrally"));
    fs::write(&config_path, toml::to_string(&central).unwrap()).unwrap();
    let mut worker = central.clone();
    worker.defaults.registry_central_url = Some("https://central.invalid".into());
    let marker = tmp.path().join("unexpected-backend-start");
    let executable = tmp.path().join("codex-fixture");
    write_executable(
        &executable,
        &format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    );
    worker.profiles.get_mut("real").unwrap().codex_path =
        Some(executable.to_string_lossy().into_owned());
    worker
        .profiles
        .get_mut("real")
        .unwrap()
        .routing
        .max_paid_reviews_per_ticket = Some(99);
    let worker_config = tmp.path().join("worker.toml");
    fs::write(&worker_config, toml::to_string(&worker).unwrap()).unwrap();
    let identity = tmp.path().join("identity.json");
    fs::write(&identity, r#"{"node_id":"worker-native-test"}"#).unwrap();
    let work = tmp.path().join("checked-job");
    let empty_worker_history = tmp.path().join("empty-worker-history.jsonl");
    fs::write(&empty_worker_history, "").unwrap();
    let checked = bin()
        .args([
            "native-handoff",
            "check",
            "--context-dir",
            input.to_str().unwrap(),
            "--expected-context",
            binding["context_sha256"].as_str().unwrap(),
            "--work-dir",
            work.to_str().unwrap(),
            "--config-path",
            worker_config.to_str().unwrap(),
        ])
        .env("GAH_COORDINATOR_IDENTITY_PATH", &identity)
        .env("GAH_LEDGER_PATH", &empty_worker_history)
        .assert()
        .success();
    let checked: Value = serde_json::from_slice(&checked.get_output().stdout).unwrap();
    assert_eq!(checked["binding"], binding);
    assert_eq!(checked["history_entries"], 1);
    assert_eq!(checked["effective_history_entries"], 1);
    assert_eq!(fs::read_to_string(&empty_worker_history).unwrap(), "");
    assert_eq!(checked["execution_started"], false);
    assert_eq!(checked["authority_refresh_required"], true);
    assert_eq!(
        fs::read_to_string(work.join("ledger.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert!(!marker.exists());
    #[cfg(target_os = "linux")]
    {
        // A verified context still cannot reserve outside its owned holder,
        // much less start a backend. Ambient manager identity is insufficient.
        bin()
            .args([
                "native-handoff",
                "host-reserve",
                "--context-dir",
                input.to_str().unwrap(),
                "--expected-context",
                binding["context_sha256"].as_str().unwrap(),
                "--work-dir",
                tmp.path().join("outside-holder").to_str().unwrap(),
                "--config-path",
                worker_config.to_str().unwrap(),
            ])
            .env("GAH_COORDINATOR_IDENTITY_PATH", &identity)
            .env_remove("INVOCATION_ID")
            .assert()
            .failure()
            .stderr(predicate::str::contains("requires a systemd holder"));
        assert!(!marker.exists());
    }
    fs::write(input.join("history-0000.jsonl"), "PRIVATE_TAMPER_CANARY").unwrap();
    let rejected = bin()
        .args([
            "native-handoff",
            "check",
            "--context-dir",
            input.to_str().unwrap(),
            "--expected-context",
            binding["context_sha256"].as_str().unwrap(),
            "--work-dir",
            tmp.path().join("rejected").to_str().unwrap(),
            "--config-path",
            worker_config.to_str().unwrap(),
        ])
        .env("GAH_COORDINATOR_IDENTITY_PATH", &identity)
        .assert()
        .failure();
    let error = String::from_utf8_lossy(&rejected.get_output().stderr);
    assert!(error.contains("does not match its manifest"));
    assert!(!error.contains("PRIVATE_TAMPER_CANARY"));
    assert!(!marker.exists());
}
