//! Exercise the fake executable with production argv, not its Python internals.
use crate::support::{forge::FakeForge, scenario::ScenarioHarness, test_tempdir, ExecGuard};
use serde_json::{json, Value};
use std::process::Stdio;

#[path = "commands.rs"]
mod commands;

fn call(forge: &FakeForge, cli: &str, args: &[&str]) -> String {
    let out = forge.command(cli).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn api(forge: &FakeForge, cli: &str, args: &[&str]) -> Value {
    serde_json::from_str(&call(forge, cli, args)).unwrap()
}

#[test]
fn forge_github_lifecycle_matches_provider_argv_and_json() {
    let _guard = ExecGuard::new();
    let tmp = test_tempdir();
    let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
    let issue = api(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "POST",
            "repos/owner/repo/issues",
            "-f",
            "title=Tracking",
            "-f",
            "body=acceptance",
            "-f",
            "labels[]=autonomous",
        ],
    );
    assert_eq!(issue["number"], 1);
    assert_eq!(issue["labels"], json!([{"name":"autonomous"}]));
    forge.update(|s| {
        s["branches"]["gah/feature"] = json!("head-1");
    });
    assert_eq!(
        call(
            &forge,
            "gh",
            &[
                "pr",
                "create",
                "--repo",
                "owner/repo",
                "--base",
                "main",
                "--head",
                "gah/feature",
                "--title",
                "Draft: change",
                "--body",
                "Closes #1",
                "--draft"
            ]
        ),
        "https://github.com/owner/repo/pull/2"
    );
    let pulls = api(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "GET",
            "repos/owner/repo/pulls",
            "-f",
            "state=all",
            "-f",
            "head=owner:gah/feature",
        ],
    );
    assert_eq!(
        pulls[0]["head"],
        json!({"ref":"gah/feature","sha":"head-1"})
    );
    forge.update(|s| {
        s["checks"]["head-1"] = json!([{"status":"completed","conclusion":"success"}]);
    });
    let checks = api(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "GET",
            "repos/owner/repo/commits/head-1/check-runs?per_page=100",
        ],
    );
    assert_eq!(checks["total_count"], 1);
    let view = api(&forge, "gh", &["pr", "view", "2", "--repo", "owner/repo", "--json",
        "number,url,title,body,isDraft,headRefName,baseRefName,state,mergedAt,headRefOid,statusCheckRollup"]);
    assert_eq!(
        view["statusCheckRollup"],
        json!([{"status":"COMPLETED","conclusion":"SUCCESS"}])
    );
    // A new head must not inherit the old head's successful checks.
    forge.update(|s| {
        s["pulls"]["2"]["sha"] = json!("head-2");
    });
    assert_eq!(
        api(
            &forge,
            "gh",
            &["pr", "view", "2", "--json", "statusCheckRollup"]
        ),
        json!({"statusCheckRollup":[]})
    );
    call(
        &forge,
        "gh",
        &["pr", "review", "2", "--request-changes", "--body", "fix it"],
    );
    assert_eq!(
        api(
            &forge,
            "gh",
            &["api", "--method", "GET", "repos/owner/repo/pulls/2/reviews"]
        )[0]["commit_id"],
        "head-2"
    );
    call(&forge, "gh", &["pr", "ready", "2", "--repo", "owner/repo"]);
    let failed = forge
        .command("gh")
        .args([
            "pr",
            "merge",
            "2",
            "--squash",
            "--match-head-commit",
            "head-1",
        ])
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert_eq!(forge.state()["pulls"]["2"]["state"], "open");
    call(
        &forge,
        "gh",
        &[
            "pr",
            "merge",
            "2",
            "--squash",
            "--admin",
            "--match-head-commit",
            "head-2",
            "--delete-branch",
            "--repo",
            "owner/repo",
        ],
    );
    let merged = api(
        &forge,
        "gh",
        &[
            "pr",
            "list",
            "--state",
            "all",
            "--json",
            "state,mergedAt,mergeCommit",
        ],
    );
    assert_eq!(merged[0]["state"], "MERGED");
    assert!(merged[0]["mergedAt"].is_string());
    assert_eq!(
        forge.state()["branches"]["main"],
        merged[0]["mergeCommit"]["oid"]
    );
    assert!(forge.state()["branches"].get("gah/feature").is_none());
    call(
        &forge,
        "gh",
        &["issue", "close", "1", "--repo", "owner/repo"],
    );
    assert_eq!(
        call(
            &forge,
            "gh",
            &["api", "repos/owner/repo/issues/1", "--jq", ".state"]
        ),
        "closed"
    );
    assert_eq!(
        forge.calls()[0],
        vec![
            "gh",
            "api",
            "--method",
            "POST",
            "repos/owner/repo/issues",
            "-f",
            "title=Tracking",
            "-f",
            "body=acceptance",
            "-f",
            "labels[]=autonomous"
        ]
    );
}

#[test]
fn forge_claims_comments_labels_and_pagination_match_recorded_shapes() {
    let _guard = ExecGuard::new();
    let tmp = test_tempdir();
    let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
    forge.pull(7, "gah/change", "head", "change");
    assert_eq!(
        call(&forge, "gh", &["api", "user", "--jq", ".login"]),
        "gah-bot"
    );
    assert_eq!(
        call(
            &forge,
            "gh",
            &[
                "api",
                "--method",
                "POST",
                "repos/owner/repo/issues/7/assignees",
                "-f",
                "assignees[]=gah-bot",
                "--jq",
                ".assignees[].login"
            ]
        ),
        "gah-bot"
    );
    let body = "<!-- gah-claim login=gah-bot -->\nquoted ' \" verdict";
    let posted = api(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "POST",
            "repos/owner/repo/issues/7/comments",
            "--raw-field",
            &format!("body={body}"),
        ],
    );
    let query = ".[] | select(.body | startswith(\"<!-- gah-claim \")) | {id, login: (.user.login // \"\"), created_at, updated_at}";
    let claim = api(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "GET",
            "--paginate",
            "repos/owner/repo/issues/7/comments?per_page=100",
            "--jq",
            query,
        ],
    );
    assert_eq!(claim["id"], posted["id"]);
    assert_eq!(claim["login"], "gah-bot");
    forge.update(|s| s["now"] = json!("2099-01-01T00:00:00Z"));
    let endpoint = format!("repos/owner/repo/issues/comments/{}", posted["id"]);
    call(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "PATCH",
            &endpoint,
            "-f",
            "body=renewed",
            "--silent",
        ],
    );
    let listed = api(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "GET",
            "repos/owner/repo/issues/7/comments",
            "-f",
            "per_page=100",
        ],
    );
    assert_eq!(listed[0]["body"], "renewed");
    assert_ne!(listed[0]["created_at"], listed[0]["updated_at"]);
    for body in ["verdict", "third"] {
        call(
            &forge,
            "gh",
            &[
                "api",
                "--method",
                "POST",
                "repos/owner/repo/issues/7/comments",
                "--raw-field",
                &format!("body={body}"),
            ],
        );
    }
    assert_eq!(
        api(
            &forge,
            "gh",
            &[
                "api",
                "--method",
                "GET",
                "repos/owner/repo/issues/7/comments?per_page=1&page=2"
            ]
        )[0]["body"],
        "verdict"
    );
    let pages = call(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "GET",
            "--paginate",
            "repos/owner/repo/issues/7/comments?per_page=1",
        ],
    );
    let pages: Vec<Value> = serde_json::Deserializer::from_str(&pages)
        .into_iter()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(pages.len(), 3);
    assert!(pages.iter().all(|page| page.as_array().unwrap().len() == 1));
    call(
        &forge,
        "gh",
        &[
            "api",
            "repos/owner/repo/issues/7/labels",
            "-f",
            "labels[]=gah-needs-fix",
            "-f",
            "labels[]=custom",
        ],
    );
    call(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "DELETE",
            "repos/owner/repo/issues/7/labels/gah-needs-fix",
        ],
    );
    assert_eq!(
        call(
            &forge,
            "gh",
            &[
                "api",
                "repos/owner/repo/issues/7/labels",
                "--jq",
                ".[].name"
            ]
        ),
        "custom"
    );
    call(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "DELETE",
            "repos/owner/repo/issues/7/assignees",
            "-f",
            "assignees[]=gah-bot",
            "--jq",
            ".assignees[].login",
        ],
    );
    assert_eq!(forge.state()["pulls"]["7"]["assignees"], json!([]));
    call(&forge, "gh", &["api", "--method", "DELETE", &endpoint]);
    assert_eq!(
        forge.state()["pulls"]["7"]["comments"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(!forge
        .command("gh")
        .args(["api", "repos/owner/repo/unknown"])
        .output()
        .unwrap()
        .status
        .success());
}

#[test]
fn forge_gitlab_lifecycle_and_structured_discussions() {
    let _guard = ExecGuard::new();
    let tmp = test_tempdir();
    let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
    forge.update(|s| s["branches"]["gah/change"] = json!("source"));
    let issue = api(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/issues",
            "--hostname",
            "gitlab.com",
            "--method",
            "POST",
            "--raw-field",
            "title=Tracking",
            "--raw-field",
            "description=acceptance",
        ],
    );
    assert_eq!(issue["iid"], 1);
    let mr = api(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/merge_requests",
            "--hostname",
            "gitlab.com",
            "--method",
            "POST",
            "--raw-field",
            "source_branch=gah/change",
            "--raw-field",
            "target_branch=main",
            "--raw-field",
            "title=Draft: change",
            "--raw-field",
            "description=Closes #1",
        ],
    );
    assert_eq!(mr["iid"], 1);
    assert_eq!(mr["sha"], "source");
    forge
        .update(|s| s["pipelines"]["source"] = json!([{"id":1,"sha":"source","status":"success"}]));
    let rows = api(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/merge_requests?state=opened&source_branch=gah%2Fchange",
            "--hostname",
            "gitlab.com",
            "--method",
            "GET",
        ],
    );
    assert_eq!(rows[0]["head_pipeline"]["sha"], "source");
    let pipelines = api(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/merge_requests/1/pipelines",
            "--method",
            "GET",
            "--raw-field",
            "per_page=100",
        ],
    );
    assert_eq!(pipelines[0]["status"], "success");
    call(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/merge_requests/1/notes",
            "--method",
            "POST",
            "--raw-field",
            "body=NEEDS_FIX verdict",
        ],
    );
    let mut child = forge
        .command("glab")
        .args([
            "api",
            "projects/42/merge_requests/1/discussions",
            "--hostname",
            "gitlab.com",
            "--method",
            "POST",
            "--input",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"body":"positioned","position":{"head_sha":"source"}}"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let discussion: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(discussion["individual_note"], false);
    assert_eq!(discussion["notes"][0]["type"], "DiffNote");
    assert_eq!(
        discussion["notes"][0]["position"],
        json!({"head_sha":"source"})
    );
    assert_eq!(discussion["notes"][0]["resolvable"], true);
    assert_eq!(discussion["notes"][0]["resolved"], false);
    let endpoint = format!(
        "projects/42/merge_requests/1/discussions/{}",
        discussion["id"].as_str().unwrap()
    );
    call(
        &forge,
        "glab",
        &[
            "api",
            &format!("{endpoint}/notes"),
            "--method",
            "POST",
            "--raw-field",
            "body=Addressed",
        ],
    );
    call(
        &forge,
        "glab",
        &["api", &endpoint, "--method", "PUT", "-F", "resolved=true"],
    );
    let resolved = api(&forge, "glab", &["api", &endpoint, "--method", "GET"]);
    assert_eq!(resolved["notes"][0]["resolved"], true);
    assert_eq!(resolved["notes"][1]["body"], "Addressed");
    assert_eq!(resolved["notes"][1]["type"], "DiscussionNote");
    assert_eq!(resolved["notes"][1]["resolvable"], true);
    assert_eq!(resolved["notes"][1]["resolved"], true);
    call(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/merge_requests/1",
            "--method",
            "PUT",
            "--raw-field",
            "title=change",
            "--raw-field",
            "add_labels=gah-ready-for-human",
            "--raw-field",
            "assignee_ids=1",
        ],
    );
    call(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/merge_requests/1/merge",
            "--method",
            "PUT",
            "--raw-field",
            "squash=true",
            "--raw-field",
            "sha=source",
            "--raw-field",
            "should_remove_source_branch=true",
        ],
    );
    assert_eq!(
        api(
            &forge,
            "glab",
            &["api", "projects/42/merge_requests/1", "--method", "GET"]
        )["state"],
        "merged"
    );
    call(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/issues/1",
            "--method",
            "PUT",
            "--raw-field",
            "state_event=close",
        ],
    );
    assert_eq!(forge.state()["issues"]["1"]["state"], "closed");
}

#[test]
fn scenario_forge_retains_mutations_across_loop_iterations() {
    for (provider, cli) in [("github", "gh"), ("gitlab", "glab")] {
        let mut harness = ScenarioHarness::new(provider).with_config_append(
            "provider_api_base = \"https://gitlab.com/api/v4\"\nprovider_project_id = \"42\"\n",
        );
        let forge = harness.stateful_forge();
        // No worker should be selected for an already merged PR.
        forge.pull(1, "gah/done", "head", "TICKET-1 done");
        call(forge, "gh", &["pr", "ready", "1"]);
        call(forge, "gh", &["pr", "merge", "1", "--squash"]);
        let first = harness.run_one_loop().unwrap();
        assert_eq!(first.exit_code, Some(0), "{first:?}");
        harness
            .stateful_forge()
            .update(|s| s["pulls"]["1"]["body"] = json!("edited between loops"));
        let second = harness.run_one_loop().unwrap();
        assert_eq!(second.exit_code, Some(0), "{second:?}");
        let forge = harness.stateful_forge();
        assert_eq!(forge.state()["pulls"]["1"]["body"], "edited between loops");
        assert_eq!(forge.state()["pulls"]["1"]["merge_method"], "squash");
        assert!(second.call_counts[cli] > first.call_counts[cli]);
    }
}

#[test]
fn forge_parallel_comments_keep_unique_ids_and_all_writes() {
    let _guard = ExecGuard::new();
    let tmp = test_tempdir();
    let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
    forge.pull(1, "gah/change", "head", "change");
    std::thread::scope(|scope| {
        for i in 0..8 {
            let forge = &forge;
            scope.spawn(move || {
                call(
                    forge,
                    "gh",
                    &[
                        "api",
                        "--method",
                        "POST",
                        "repos/owner/repo/issues/1/comments",
                        "--raw-field",
                        &format!("body=comment-{i}"),
                    ],
                );
            });
        }
    });
    let state = forge.state();
    let comments = state["pulls"]["1"]["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 8);
    let ids: std::collections::BTreeSet<_> =
        comments.iter().map(|c| c["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), 8);
    assert_eq!(forge.calls().len(), 9);
}

#[test]
fn forge_native_relationships_branches_and_auto_merge() {
    let _guard = ExecGuard::new();
    let tmp = test_tempdir();
    let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
    for title in ["Parent", "Child"] {
        call(
            &forge,
            "gh",
            &[
                "api",
                "--method",
                "POST",
                "repos/owner/repo/issues",
                "-f",
                &format!("title={title}"),
            ],
        );
    }
    let child_id = forge.state()["issues"]["2"]["id"].to_string();
    call(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "POST",
            "repos/owner/repo/issues/1/sub_issues",
            "-F",
            &format!("sub_issue_id={child_id}"),
        ],
    );
    assert_eq!(
        api(
            &forge,
            "gh",
            &[
                "api",
                "--method",
                "GET",
                "repos/owner/repo/issues/1/sub_issues",
                "--paginate",
                "-f",
                "per_page=100"
            ]
        )[0]["number"],
        2
    );
    call(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/issues/1/links",
            "--method",
            "POST",
            "--raw-field",
            "target_project_id=42",
            "--raw-field",
            "target_issue_iid=2",
            "--raw-field",
            "link_type=blocks",
        ],
    );
    assert_eq!(
        api(
            &forge,
            "glab",
            &["api", "projects/42/issues/2/links", "--method", "GET"]
        )[0]["link_type"],
        "is_blocked_by"
    );
    call(
        &forge,
        "gh",
        &[
            "api",
            "--method",
            "POST",
            "repos/owner/repo/git/refs",
            "-f",
            "ref=refs/heads/gah/change",
            "-f",
            "sha=head",
        ],
    );
    assert_eq!(
        api(
            &forge,
            "gh",
            &[
                "api",
                "--method",
                "GET",
                "repos/owner/repo/git/ref/heads/gah/change"
            ]
        )["object"]["sha"],
        "head"
    );
    call(
        &forge,
        "glab",
        &[
            "api",
            "projects/42/merge_requests",
            "--method",
            "POST",
            "--raw-field",
            "source_branch=gah/change",
            "--raw-field",
            "title=Ready",
        ],
    );
    let stale = forge
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
            "sha=old-head",
        ])
        .output()
        .unwrap();
    assert!(!stale.status.success());
    assert!(forge.state()["pulls"]["1"].get("auto_merge").is_none());
    call(
        &forge,
        "glab",
        &[
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
            "--raw-field",
            "should_remove_source_branch=true",
        ],
    );
    assert!(forge.state()["pulls"]["1"]["merged_at"].is_null());
    forge.update(|s| s["pipelines"]["head"] = json!([{"id":1,"sha":"head","status":"success"}]));
    assert_eq!(
        api(
            &forge,
            "glab",
            &["api", "projects/42/merge_requests/1", "--method", "GET"]
        )["state"],
        "merged"
    );
}

#[test]
fn forge_creation_preserves_seeded_history_and_allocates_unused_numbers() {
    let _guard = ExecGuard::new();
    for provider in ["gh", "glab"] {
        let tmp = test_tempdir();
        let forge = FakeForge::new(&tmp.path().join("forge"), &tmp.path().join("bin"));
        forge.pull(1, "gah/seeded", "head", "seeded");
        forge.update(|s| {
            let pull = &mut s["pulls"]["1"];
            pull["comments"] = json!([{"id":10,"body":"keep this note"}]);
            pull["reviews"] = json!([{"id":11,"state":"CHANGES_REQUESTED"}]);
            pull["review_comments"] = json!([{"id":12,"body":"inline finding"}]);
            pull["discussions"] = json!([{"id":"thread","individual_note":false,"notes":[]}]);
            pull["versions"] = json!([{"id":13,"head_commit_sha":"head"}]);
            pull["labels"] = json!(["gah-needs-fix"]);
            pull["assignees"] = json!(["gah-bot"]);
        });
        let seeded = forge.state()["pulls"]["1"].clone();
        for branch in ["gah/seeded", "gah/second"] {
            if provider == "gh" {
                call(
                    &forge,
                    provider,
                    &[
                        "pr",
                        "create",
                        "--head",
                        branch,
                        "--title",
                        "published",
                        "--body",
                        "description",
                    ],
                );
            } else {
                call(
                    &forge,
                    provider,
                    &[
                        "api",
                        "projects/42/merge_requests",
                        "--method",
                        "POST",
                        "--raw-field",
                        &format!("source_branch={branch}"),
                        "--raw-field",
                        "title=published",
                        "--raw-field",
                        "description=description",
                    ],
                );
            }
            let state = forge.state();
            let published = &state["pulls"]["1"];
            for key in [
                "id",
                "number",
                "comments",
                "reviews",
                "review_comments",
                "discussions",
                "versions",
                "labels",
                "assignees",
                "state",
                "merged_at",
                "merge_sha",
            ] {
                assert_eq!(published[key], seeded[key], "{provider}: {key}");
            }
            assert_eq!(published["title"], "published");
            assert_eq!(published["body"], "description");
        }
        let state = forge.state();
        assert_eq!(state["pulls"].as_object().unwrap().len(), 2);
        assert_eq!(state["pulls"]["2"]["branch"], "gah/second");
        assert_ne!(state["pulls"]["1"]["id"], state["pulls"]["2"]["id"]);
    }
}

#[test]
fn forge_gitlab_discussion_reaches_production_fix_discovery() {
    let branch = "gah/discussion-fix";
    let mut harness = ScenarioHarness::new("gitlab")
        .worker_scenario("success")
        .with_config_append(
            "provider_api_base = \"https://gitlab.com/api/v4\"\nprovider_project_id = \"42\"\n\
             [profiles.test.publishing]\ntrusted_issue_human_authors = [\"gah-bot\"]\n\
             allow_pull_request_creation = false\nallow_commit_message_generation = false\n",
        );
    harness.create_remote_branch(branch);
    let worker_root = test_tempdir();
    let worker = crate::support::FakeBackend::new(worker_root.path(), "codex");
    crate::support::write_executable(
        &worker.bin_dir().join("codex"),
        "#!/bin/sh\nprintf 'fixed\\n' > repaired.txt\n",
    );
    harness.install_custom_worker("codex", &worker);
    let forge = harness.stateful_forge();
    forge.pull(1, branch, "head", "change");
    call(forge, "gh", &["pr", "ready", "1"]);
    let thread = api(
        forge,
        "glab",
        &[
            "api",
            "projects/42/merge_requests/1/discussions",
            "--method",
            "POST",
            "--raw-field",
            "body=Seeded finding must reach the repair worker",
        ],
    );
    assert_eq!(thread["individual_note"], false);
    assert_eq!(thread["notes"][0]["type"], "DiscussionNote");
    assert_eq!(thread["notes"][0]["resolvable"], true);
    assert_eq!(thread["notes"][0]["resolved"], false);
    let original = std::env::var_os("GITLAB_PAT2");
    std::env::set_var("GITLAB_PAT2", "fake-forge-token");
    let result = harness.run_dispatch(&[
        "--mode",
        "fix",
        "--existing-branch",
        branch,
        "--backend",
        "codex",
    ]);
    match original {
        Some(value) => std::env::set_var("GITLAB_PAT2", value),
        None => std::env::remove_var("GITLAB_PAT2"),
    }
    let result = result.unwrap();
    assert_eq!(result.exit_code, Some(0), "{result:?}");
    let entries = crate::support::fake_ledger::TestLedger::read_from(&harness.ledger_path).unwrap();
    let session = entries.last().unwrap()["session_dir"].as_str().unwrap();
    let prompt =
        std::fs::read_to_string(std::path::Path::new(session).join("attempt-1/task.md")).unwrap();
    assert!(prompt.contains("Unresolved GitLab discussions"), "{prompt}");
    assert!(
        prompt.contains("Seeded finding must reach the repair worker"),
        "{prompt}"
    );
    assert!(harness.stateful_forge().calls().iter().any(|args| args
        .iter()
        .any(|arg| arg == "projects/42/merge_requests/1/discussions?per_page=100&page=1")));
}
