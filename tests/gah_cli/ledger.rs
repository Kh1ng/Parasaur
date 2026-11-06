use super::*;

#[test]
fn ledger_receive_dispatch_replays_once_and_refuses_a_changed_worker_result() {
    use git_agent_harness::ledger::remote_dispatch::{
        DispatchBinding, DispatchReceipt, ProcessTermination, RemoteExecution, TerminationReason,
    };
    use git_agent_harness::ledger::LedgerEntry;
    let tmp = test_tempdir();
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    let cfg_path = write_real_repo_config(&tmp, &repo, "github");
    let cfg: git_agent_harness::config::GahConfig =
        toml::from_str(&fs::read_to_string(&cfg_path).unwrap()).unwrap();
    let profile = &cfg.profiles["real"];
    let binding = DispatchBinding {
        run_id: uuid::Uuid::new_v4().to_string(),
        context_sha256: "a".repeat(64),
        node_id: "worker-one".into(),
        profile: "real".into(),
        repo_id: profile.repo_id.clone(),
        repo: profile.repo.clone(),
        work_id: "#1".into(),
        mode: "fix".into(),
        branch: None,
        review_generation: None,
        dispatch_reason: Some("initial".into()),
        backend: "codex".into(),
        backend_instance: None,
        model: None,
    };
    let mut entry = LedgerEntry::new(
        "real",
        profile,
        "codex",
        "fix",
        "fixture",
        Some(binding.run_id.clone()),
        None,
    );
    entry.work_id = Some(binding.work_id.clone());
    entry.dispatch_reason = binding.dispatch_reason.clone();
    entry.validation_result = Some("failed".into());
    let mut receipt = DispatchReceipt {
        context_sha256: binding.context_sha256.clone(),
        execution: RemoteExecution {
            node_id: binding.node_id.clone(),
            termination: ProcessTermination {
                reason: TerminationReason::Exited,
                exit_code: Some(1),
                signal: None,
            },
        },
        ledger: entry,
    };
    let binding_file = tmp.path().join("prepared-binding.json");
    let receipt_file = tmp.path().join("worker-receipt.json");
    let ledger_path = tmp.path().join("ledger.jsonl");
    let historical = LedgerEntry::new(
        "real",
        profile,
        "codex",
        "fix",
        "older",
        Some(uuid::Uuid::new_v4().to_string()),
        None,
    );
    let history = format!("{}\n", serde_json::to_string(&historical).unwrap());
    fs::write(&ledger_path, &history).unwrap();
    fs::write(&binding_file, serde_json::to_vec(&binding).unwrap()).unwrap();
    fs::write(&receipt_file, serde_json::to_vec(&receipt).unwrap()).unwrap();
    for expected in ["recorded", "already_recorded"] {
        bin()
            .args([
                "ledger",
                "receive-dispatch",
                "--binding-file",
                binding_file.to_str().unwrap(),
                "--receipt-file",
                receipt_file.to_str().unwrap(),
                "--config-path",
                cfg_path.to_str().unwrap(),
            ])
            .env("GAH_LEDGER_PATH", &ledger_path)
            .assert()
            .success()
            .stdout(format!("\"{expected}\"\n"));
    }
    let saved = fs::read_to_string(&ledger_path).unwrap();
    assert!(saved.starts_with(&history));
    assert_eq!(saved.lines().count(), 2);
    let stored: Value = serde_json::from_str(saved.lines().last().unwrap()).unwrap();
    assert_eq!(stored["validation_result"], "failed");
    assert_eq!(stored["remote_execution"]["termination"]["exit_code"], 1);
    assert_eq!(stored["usage"]["actual_cost_usd"], Value::Null);
    receipt.ledger.usage.actual_cost_usd = Some(20.0);
    fs::write(&receipt_file, serde_json::to_vec(&receipt).unwrap()).unwrap();
    bin()
        .args([
            "ledger",
            "receive-dispatch",
            "--binding-file",
            binding_file.to_str().unwrap(),
            "--receipt-file",
            receipt_file.to_str().unwrap(),
            "--config-path",
            cfg_path.to_str().unwrap(),
        ])
        .env("GAH_LEDGER_PATH", &ledger_path)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "A different terminal result already owns this native run",
        ));
    assert_eq!(fs::read_to_string(&ledger_path).unwrap(), saved);
    let canary = "PRIVATE_WORKER_INPUT_CANARY";
    let mut invalid = serde_json::to_value(&receipt).unwrap();
    invalid["execution"]["termination"]["reason"] = serde_json::json!(canary);
    fs::write(&receipt_file, serde_json::to_vec(&invalid).unwrap()).unwrap();
    let rejected = bin()
        .args([
            "ledger",
            "receive-dispatch",
            "--binding-file",
            binding_file.to_str().unwrap(),
            "--receipt-file",
            receipt_file.to_str().unwrap(),
            "--config-path",
            cfg_path.to_str().unwrap(),
        ])
        .env("GAH_LEDGER_PATH", &ledger_path)
        .assert()
        .failure();
    let error = String::from_utf8_lossy(&rejected.get_output().stderr);
    assert!(error.contains("Native dispatch input does not match the supported schema"));
    assert!(!error.contains(canary));
    assert_eq!(fs::read_to_string(ledger_path).unwrap(), saved);
}

#[test]
fn ledger_summary_json_outputs_machine_readable_counts() {
    let tmp = test_tempdir();
    let ledger_path = tmp.path().join("ledger.jsonl");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo);
    let cfg = write_real_repo_config(&tmp, &repo, "github");

    fs::write(&ledger_path, "").unwrap();
    let out = bin()
        .args([
            "ledger",
            "summary",
            "--since",
            "7d",
            "--json",
            "--config-path",
            cfg.to_str().unwrap(),
        ])
        .env("GAH_LEDGER_PATH", ledger_path.to_str().unwrap())
        .assert()
        .success();
    let parsed: Value =
        serde_json::from_slice(&out.get_output().stdout).expect("summary output must be JSON");
    assert_eq!(parsed["entries"], 0);
    assert!(parsed["by_mode"].is_object());
}

#[test]
fn ledger_work_no_matching_entry_reports_empty_result() {
    let tmp = test_tempdir();
    let ledger_path = tmp.path().join("ledger.jsonl");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo);
    let cfg = write_real_repo_config(&tmp, &repo, "github");

    fs::write(&ledger_path, "").unwrap();
    bin()
        .args([
            "ledger",
            "work",
            "TICKET-999",
            "--config-path",
            cfg.to_str().unwrap(),
        ])
        .env("GAH_LEDGER_PATH", ledger_path.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "No ledger entries found for work item 'TICKET-999'.",
        ));
}

#[test]
fn ledger_clear_attempts_dry_run_stays_pretended_stateful() {
    let tmp = test_tempdir();
    let ledger_path = tmp.path().join("ledger.jsonl");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo);
    let cfg = write_real_repo_config(&tmp, &repo, "github");

    fs::write(&ledger_path, "").unwrap();
    bin()
        .args([
            "ledger",
            "clear-attempts",
            "--profile",
            "real",
            "--dry-run",
            "TICKET-999",
            "--config-path",
            cfg.to_str().unwrap(),
        ])
        .env("GAH_LEDGER_PATH", ledger_path.to_str().unwrap())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Dry run: would append tombstone entry for work_id 'TICKET-999':",
        ));
}
