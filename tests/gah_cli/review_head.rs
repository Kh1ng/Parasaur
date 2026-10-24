use super::*;
use std::path::Path;

#[test]
fn review_unchanged_head_skips_locally_and_across_nodes_after_policy_check() {
    let tmp = test_tempdir();
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo);
    add_origin_and_feature_commit(&repo);
    checkout_branch(&repo, "main");
    let cfg = write_real_repo_config_with_extra(
        &tmp,
        &repo,
        "github",
        "[profiles.real.routing]\nreview_backend = 'claude'\n",
        "",
    );
    let backend = FakeBackend::new(tmp.path(), "claude");
    backend.install_sequence(vec![Scenario::success().with_stdout(
        r#"Review notes
{"verdict":"APPROVE","confidence":"high","human_required":false,"blocking_findings":[],"non_blocking_findings":[],"risk_notes":[],"evidence":["file:src.txt"]}"#,
    )]);
    make_fake_github_review_api(backend.bin_dir());
    let gh = backend.bin_dir().join("gh");
    let script = fs::read_to_string(&gh).unwrap().replace(
        "printf '[]\\n'",
        "if [ -f \"${0%/*}/posted\" ]; then cat \"${0%/*}/receipt.json\"; else printf '[]\\n'; fi",
    ).replace(
        "\"api --method POST repos/owner/real/issues/7/comments\") exit 0",
        "\"api --method POST repos/owner/real/issues/7/comments\") for arg; do case \"$arg\" in body=*) printf '%s' \"${arg#body=}\" > \"${0%/*}/posted\" ;; esac; done; exit 0",
    );
    make_fake_bin_with_body(backend.bin_dir(), "gh", &script);
    let output = ProcessCommand::new("git")
        .args(["rev-parse", "feature/review"])
        .current_dir(&repo)
        .output()
        .unwrap();
    let head = String::from_utf8(output.stdout).unwrap().trim().to_string();
    let receipt = format!(
        "GAH review verdict: `APPROVE`\n<!-- gah-review-head-v1 {} -->\n",
        serde_json::json!({"head":head,"backend":"claude","model":null})
    );
    fs::write(
        backend.bin_dir().join("receipt.json"),
        serde_json::to_string(&serde_json::json!([{"body":receipt}])).unwrap(),
    )
    .unwrap();
    let ledger = tmp.path().join("ledger.jsonl");
    let run = || {
        let mut command = bin();
        command
            .args([
                "dispatch",
                "--profile",
                "real",
                "--mode",
                "review",
                "--branch",
                "feature/review",
                "--config-path",
                cfg.to_str().unwrap(),
            ])
            .env("PATH", prepend_path(backend.bin_dir()))
            .env("GAH_LEDGER_PATH", &ledger);
        command
    };
    run().assert().success();
    let posted = fs::read_to_string(backend.bin_dir().join("posted")).unwrap();
    assert!(posted.contains("<!-- gah-review-head-v1 "));
    assert!(posted.contains(&head));
    run()
        .assert()
        .success()
        .stdout(predicate::str::contains("Skipping already reviewed head"));
    let entries: Vec<Value> = fs::read_to_string(&ledger)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let skipped = entries.last().unwrap();
    assert_eq!(skipped["review_source_sha"], head);
    assert_eq!(skipped["validation_result"], "skipped_duplicate_review");
    assert_eq!(skipped["attempts"], serde_json::json!([]));
    fs::remove_file(&ledger).unwrap();
    run()
        .assert()
        .success()
        .stdout(predicate::str::contains("Skipping already reviewed head"));
    fs::remove_file(backend.bin_dir().join("receipt.json")).unwrap();
    run()
        .assert()
        .success()
        .stdout(predicate::str::contains("Skipping already reviewed head"));
    let policy = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/policy-read-only.toml");
    let config = fs::read_to_string(&cfg).unwrap().replace(
        "[profiles.real]",
        &format!("[profiles.real]\npolicy_path = '{}'", policy.display()),
    );
    fs::write(&cfg, config).unwrap();
    run()
        .assert()
        .failure()
        .stderr(predicate::str::contains("POLICY BLOCKED"));
    assert_eq!(backend.call_count(), 1);
}
