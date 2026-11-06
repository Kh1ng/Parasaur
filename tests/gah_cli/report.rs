use super::*;

#[test]
fn project_and_work_item_grouping_reaches_real_report_and_ledger_cli() {
    use git_agent_harness::ledger::{AttemptRecord, LedgerEntry, LedgerUsage};
    let tmp = test_tempdir();
    let ledger_path = tmp.path().join("ledger.jsonl");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo);
    let cfg = write_real_repo_config(&tmp, &repo, "github");
    let loaded = support::load_config(Some(cfg.to_str().unwrap())).unwrap();
    let mut entry = LedgerEntry::new(
        "real",
        loaded.profiles.get("real").unwrap(),
        "codex",
        "fix",
        "task",
        None,
        None,
    );
    entry.repo = "owner/repo".into();
    entry.work_id = Some("#42".into());
    entry.usage.total_tokens = Some(999);
    entry.attempts = vec![AttemptRecord {
        attempt_number: 1,
        backend: "codex".into(),
        usage: LedgerUsage {
            total_tokens: Some(12),
            ..Default::default()
        },
        ..Default::default()
    }];
    fs::write(
        &ledger_path,
        format!("{}\n", serde_json::to_string(&entry).unwrap()),
    )
    .unwrap();
    for (grouping, field, label) in [
        ("project", "grouped_by_project", "Project"),
        ("work-item", "grouped_by_work_item", "Work item"),
    ] {
        let key = if grouping == "project" {
            serde_json::json!(["real", "github", "owner/repo"])
        } else {
            serde_json::json!(["real", "github", "owner/repo", "#42"])
        }
        .to_string();
        for command in [vec!["report"], vec!["ledger", "summary"]] {
            let args: Vec<_> = command
                .iter()
                .copied()
                .chain([
                    "--since",
                    "7d",
                    "--group-by",
                    grouping,
                    "--config-path",
                    cfg.to_str().unwrap(),
                ])
                .collect();
            let output = bin()
                .args(&args)
                .arg("--json")
                .env("GAH_LEDGER_PATH", &ledger_path)
                .assert()
                .success();
            let value: Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
            let row = if command[0] == "report" {
                assert_eq!(value["group_by"], label);
                assert_eq!(value["comparisons"][0]["backend_or_model"], key);
                &value["comparisons"][0]
            } else {
                assert_eq!(value[field][0]["group_key"], key);
                &value[field][0]
            };
            assert_eq!(row["total_tokens"], 12);
            assert_eq!(row["attempts"], 1);
            assert!(row["actual_cost_usd"].is_null());
            let plain = bin()
                .args(args)
                .env("GAH_LEDGER_PATH", &ledger_path)
                .assert()
                .success();
            assert!(String::from_utf8_lossy(&plain.get_output().stdout).contains(&key));
        }
    }
}

#[test]
fn report_json_is_supported_with_empty_ledger() {
    let tmp = test_tempdir();
    let ledger_path = tmp.path().join("ledger.jsonl");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo);
    let cfg = write_real_repo_config(&tmp, &repo, "github");

    fs::write(&ledger_path, "").unwrap();
    let out = bin()
        .args([
            "report",
            "--json",
            "--since",
            "365d",
            "--group-by",
            "backend",
            "--config-path",
            cfg.to_str().unwrap(),
        ])
        .env("GAH_LEDGER_PATH", ledger_path.to_str().unwrap())
        .assert()
        .success();

    let payload: Value =
        serde_json::from_slice(&out.get_output().stdout).expect("report output must be JSON");
    assert_eq!(payload["total_entries"], 0);
    assert_eq!(payload["comparisons"].as_array().unwrap().len(), 0);
}

#[test]
fn report_series_json_emits_empty_series_for_empty_ledger() {
    let tmp = test_tempdir();
    let ledger_path = tmp.path().join("ledger.jsonl");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo);
    let cfg = write_real_repo_config(&tmp, &repo, "github");

    fs::write(&ledger_path, "").unwrap();
    let out = bin()
        .args([
            "report",
            "--json",
            "--series",
            "--bucket",
            "daily",
            "--since",
            "365d",
            "--config-path",
            cfg.to_str().unwrap(),
        ])
        .env("GAH_LEDGER_PATH", ledger_path.to_str().unwrap())
        .assert()
        .success();

    let payload: Value = serde_json::from_slice(&out.get_output().stdout)
        .expect("report series output must be JSON");
    assert_eq!(payload["bucket"], "daily");
    assert_eq!(payload["series"].as_array().unwrap().len(), 0);
}
