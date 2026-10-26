//! Additional command contracts, including argv replayed from FakeBackend.
use super::{api, call, json, test_tempdir, ExecGuard, FakeForge};
use crate::support::{FakeBackend, Scenario};

#[test]
fn forge_replays_both_cli_list_shapes_recorded_by_fake_backend() {
    let _guard = ExecGuard::new();
    let tmp = test_tempdir();
    let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
    forge.pull(7, "gah/change", "head", "change");
    for (cli, family, number_key) in [("gh", "pr", "number"), ("glab", "mr", "iid")] {
        let backend = FakeBackend::new(&tmp.path().join("recording"), cli);
        backend.install(Scenario::success());
        assert!(std::process::Command::new(backend.bin_dir().join(cli))
            .args([family, "list"])
            .status()
            .unwrap()
            .success());
        let recorded = backend.argv_for_call(1);
        let args: Vec<_> = recorded.iter().map(String::as_str).collect();
        let listed = api(&forge, cli, &args);
        assert_eq!(listed[0][number_key], 7);
        let mut expected = vec![cli.to_string()];
        expected.extend(recorded);
        assert_eq!(forge.calls().last().unwrap(), &expected);
    }
}

#[test]
fn forge_gitlab_issue_mutations_and_branch_lifecycle() {
    let _guard = ExecGuard::new();
    let tmp = test_tempdir();
    let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
    assert_eq!(api(&forge, "glab", &["api", "user"])["username"], "gah-bot");
    call(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/issues",
            "--method",
            "POST",
            "--raw-field",
            "title=Track change",
            "--raw-field",
            "labels=autonomous,custom",
        ],
    );
    let issue = "projects/42/issues/1";
    call(
        &forge,
        "glab",
        &[
            "api",
            issue,
            "--method",
            "PUT",
            "--raw-field",
            "assignee_ids=1",
            "--raw-field",
            "remove_labels=autonomous",
            "--raw-field",
            "add_labels=ready",
        ],
    );
    let edited = api(&forge, "glab", &["api", issue, "--method", "GET"]);
    assert_eq!(edited["assignees"], json!([{"id":1,"username":"gah-bot"}]));
    assert_eq!(edited["labels"], json!(["custom", "ready"]));
    let note = api(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/issues/1/notes",
            "--method",
            "POST",
            "--raw-field",
            "body=First\nquoted ' and \" text",
        ],
    );
    let endpoint = format!("projects/42/issues/1/notes/{}", note["id"]);
    call(
        &forge,
        "glab",
        &[
            "api",
            &endpoint,
            "--method",
            "PUT",
            "--raw-field",
            "body=Updated",
        ],
    );
    assert_eq!(api(&forge, "glab", &["api", &endpoint])["body"], "Updated");
    call(&forge, "glab", &["api", &endpoint, "--method", "DELETE"]);
    assert_eq!(
        api(&forge, "glab", &["api", "projects/42/issues/1/notes"]),
        json!([])
    );
    for (event, expected) in [("close", "closed"), ("reopen", "opened")] {
        call(
            &forge,
            "glab",
            &[
                "api",
                issue,
                "--method",
                "PUT",
                "--raw-field",
                &format!("state_event={event}"),
                "--raw-field",
                "assignee_ids=0",
                "--raw-field",
                "labels=retained",
            ],
        );
        let edited = api(&forge, "glab", &["api", issue]);
        assert_eq!(edited["state"], expected);
        assert_eq!(edited["assignees"], json!([]));
        assert_eq!(edited["labels"], json!(["retained"]));
    }
    forge.update(|state| state["branches"]["main"] = json!("base-sha"));
    let created = api(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/repository/branches",
            "--method",
            "POST",
            "--raw-field",
            "branch=gah/change",
            "--raw-field",
            "ref=main",
        ],
    );
    assert_eq!(created["commit"]["id"], "base-sha");
    let branch = "projects/42/repository/branches/gah%2Fchange";
    assert_eq!(api(&forge, "glab", &["api", branch])["name"], "gah/change");
    call(&forge, "glab", &["api", branch, "--method", "DELETE"]);
    assert!(forge.state()["branches"].get("gah/change").is_none());
}

#[test]
fn forge_unknown_commands_fail_and_record_the_attempt() {
    let _guard = ExecGuard::new();
    let tmp = test_tempdir();
    let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
    forge.pull(1, "gah/change", "head", "change");
    let cases: &[(&str, &[&str])] = &[
        ("gh", &["unknown"]),
        ("glab", &["unknown"]),
        ("gh", &["pr", "list", "unexpected"]),
        ("glab", &["mr", "list", "unexpected"]),
        ("gh", &["pr", "list", "--unknown"]),
        ("glab", &["api", "user", "--unknown"]),
        ("gh", &["api", "user", "--jq", ".unsupported"]),
        ("glab", &["api", "projects/42/unknown"]),
        ("gh", &["api", "repos/owner/repo/pulls/1/reviews/unknown"]),
        (
            "gh",
            &["api", "repos/owner/repo/git/unknown/heads/gah/change"],
        ),
        (
            "glab",
            &["api", "projects/42/repository/unknown/gah%2Fchange"],
        ),
    ];
    for (cli, args) in cases {
        let output = forge.command(cli).args(*args).output().unwrap();
        assert!(!output.status.success(), "accepted {cli} {args:?}");
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(forge.calls().len(), cases.len() + 1);
    assert_eq!(forge.state()["pulls"]["1"]["state"], "open");
}

#[test]
fn forge_github_rest_closed_pulls_include_merges() {
    let _guard = ExecGuard::new();
    let tmp = test_tempdir();
    let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
    forge.pull(1, "gah/change", "head", "change");
    call(&forge, "gh", &["pr", "ready", "1"]);
    call(&forge, "gh", &["pr", "merge", "1", "--squash"]);
    assert_eq!(
        api(
            &forge,
            "gh",
            &[
                "api",
                "--method",
                "GET",
                "repos/owner/repo/pulls",
                "-f",
                "state=closed",
            ]
        )[0]["number"],
        1
    );
    assert_eq!(
        api(
            &forge,
            "gh",
            &[
                "api",
                "--method",
                "GET",
                "repos/owner/repo/pulls",
                "-f",
                "state=open",
            ]
        ),
        json!([])
    );
}

#[test]
fn forge_gitlab_auto_merge_rejects_drafts_and_closed_requests_before_ci() {
    let _guard = ExecGuard::new();
    let tmp = test_tempdir();
    let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
    forge.pull(1, "gah/change", "head", "change");
    for (draft, state) in [(true, "open"), (false, "closed")] {
        forge.update(|s| {
            s["pulls"]["1"]["draft"] = json!(draft);
            s["pulls"]["1"]["state"] = json!(state);
        });
        let output = forge
            .command("glab")
            .args([
                "api",
                "projects/42/merge_requests/1/merge",
                "--method",
                "PUT",
                "--raw-field",
                "auto_merge=true",
                "--raw-field",
                "squash=true",
                "--raw-field",
                "sha=head",
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let state = forge.state();
        assert!(state["pulls"]["1"].get("auto_merge").is_none());
        assert!(state["pulls"]["1"]["merged_at"].is_null());
    }
}
