use super::*;

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
