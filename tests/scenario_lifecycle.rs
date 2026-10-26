//! Real controller transitions against a persistent fake forge.

mod support;

use serde_json::{json, Value};
use std::fs;
use std::process::Command;
use support::fake_ledger::TestLedger;
use support::scenario::{LoopResult, ScenarioHarness};
use support::{FakeBackend, Scenario};

const FINDING: &str = "Reject an expired lease before publishing the worker result";

fn harness() -> ScenarioHarness {
    let mut harness = ScenarioHarness::new("github").with_config_append(
        "validation_commands = [\"true\"]\n\
         [profiles.test.publishing]\ntrusted_issue_human_authors = [\"gah-bot\"]\n\
         [profiles.test.publishing.issue_claim]\nmode = \"github_assignee\"\nverify_seconds = 0\n\
         [profiles.test.routing]\n\
         routine_reviewer = { backend = \"codex\", model = \"default\" }\n\
         improve_candidates = [{ backend = \"agy\", model = \"test-model\", priority = 100, included_in_quota = true }]\n\
         review_candidates = [{ backend = \"codex\", model = \"default\", priority = 100, included_in_quota = true }]\n",
    );
    // Publication constructs a provider URL rather than pushing to origin.
    // Rewrite it in this repository only, so every push stays in the fixture.
    let remote = harness.local_repo_dir.parent().unwrap().join("remote.git");
    git(
        &harness,
        &[
            "config",
            &format!("url.{}.insteadOf", remote.display()),
            "https://github.com/owner/repo.git",
        ],
    );
    let log = harness.stateful_forge().log_path.clone();
    support::write_executable(
        &harness.bin_dir.join("agy"),
        &format!(
            "#!/bin/sh\nset -eu\nif [ \"$1\" = --version ]; then echo 'test 1.0'; exit 0; fi\n\
             echo WORKER >> '{}'\necho change >> implementation.txt\necho work complete\n",
            log.display(),
        ),
    );
    reviewer(&mut harness, "NEEDS_FIX");
    let output = harness
        .stateful_forge()
        .command("gh")
        .args([
            "api",
            "repos/owner/repo/issues",
            "--method",
            "POST",
            "-f",
            "title=Implement lease validation",
            "-f",
            "body=Reject expired worker leases.",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        harness.stateful_forge().state()["issues"]["1"]["state"],
        "open"
    );
    assert!(TestLedger::read_from(&harness.ledger_path)
        .unwrap()
        .is_empty());
    harness
}

fn git(harness: &ScenarioHarness, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(&harness.local_repo_dir)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn reviewer(harness: &mut ScenarioHarness, kind: &str) {
    let reviewer = FakeBackend::new(&harness.artifacts_dir, "codex");
    let findings = if kind == "NEEDS_FIX" {
        vec![
            json!({"summary":FINDING,"file":"implementation.txt","line":"1",
            "status":"confirmed","evidence":["diff:implementation.txt:lease validation missing"]}),
        ]
    } else {
        vec![]
    };
    reviewer.install(Scenario::success().with_stdout(json!({
        "verdict":kind,"confidence":"high","human_required":false,
        "actionable_findings":findings,"blocking_findings":[],
        "evidence":["file:implementation.txt", "diff:implementation.txt:lease validation inspected"],
        "non_blocking_findings":[],"risk_notes":[]
    }).to_string()));
    harness.install_custom_worker("codex", &reviewer);
}

// Carry actual ledger output into the next seed: setup_env rewrites the seed
// on each call. No review, repair, merge, or retry entry is synthesized here.
fn run(mut harness: ScenarioHarness, action: &str, exit: i32) -> (ScenarioHarness, LoopResult) {
    let result = harness.run_one_loop().unwrap();
    assert_eq!(
        result.exit_code,
        Some(exit),
        "{}\n{}",
        result.stdout,
        result.stderr_tail
    );
    let decided = result
        .events
        .iter()
        .rev()
        .find(|event| event["event_type"] == "action_decided")
        .unwrap();
    assert_eq!(
        decided["details"].as_str().unwrap().split(':').next(),
        Some(action),
        "{}\n{}",
        result.stdout,
        result.stderr_tail
    );
    let ledger = result
        .ledger_entries
        .iter()
        .cloned()
        .fold(TestLedger::new(), |ledger, entry| ledger.with_entry(entry));
    (harness.with_ledger(ledger), result)
}

fn latest(result: &LoopResult) -> &Value {
    result.ledger_entries.last().unwrap()
}

// The fake forge models API state, not Git remote objects. Observe the real
// bare remote after each push and expose that head to the next API snapshot.
fn publish_head(harness: &mut ScenarioHarness) -> String {
    let branch = harness.stateful_forge().state()["pulls"]["2"]["branch"]
        .as_str()
        .unwrap()
        .to_string();
    let sha = git(
        harness,
        &["--git-dir", "../remote.git", "rev-parse", &branch],
    );
    assert_eq!(sha.len(), 40);
    harness.stateful_forge().update(|s| {
        s["pulls"]["2"]["sha"] = json!(sha);
        s["branches"][&branch] = json!(sha);
    });
    sha
}

fn green(harness: &mut ScenarioHarness) {
    let sha = harness.stateful_forge().state()["pulls"]["2"]["sha"]
        .as_str()
        .unwrap()
        .to_string();
    harness
        .stateful_forge()
        .update(|s| s["checks"][&sha] = json!([{"status":"completed","conclusion":"success"}]));
}

fn start() -> ScenarioHarness {
    let (mut harness, started) = run(harness(), "dispatch_ticket", 0);
    assert_eq!(started.ledger_entries.len(), 1);
    let entry = latest(&started);
    assert_eq!(entry["work_id"], "#1");
    assert_eq!(entry["source_issue_number"], "1");
    for flag in ["commit_created", "push_succeeded", "mr_created"] {
        assert_eq!(entry[flag], true, "{flag}: {entry}");
    }
    assert_eq!(entry["validation_result"], "passed");
    assert!(entry["review_verdict"].is_null());
    let state = harness.stateful_forge().state();
    assert_eq!(state["issues"]["1"]["assignees"], json!(["gah-bot"]));
    assert!(state["issues"]["1"]["comments"][0]["body"]
        .as_str()
        .unwrap()
        .starts_with("<!-- gah-claim login=gah-bot "));
    assert_eq!(state["pulls"].as_object().unwrap().len(), 1);
    let log = fs::read_to_string(&harness.stateful_forge().log_path).unwrap();
    let assigned = log
        .find("--method POST repos/owner/repo/issues/1/assignees")
        .unwrap();
    let claimed = log
        .find("--method POST repos/owner/repo/issues/1/comments")
        .unwrap();
    let worked = log.find("WORKER").unwrap();
    let published = log.find("pr create").unwrap();
    assert!(
        assigned < claimed && claimed < worked && worked < published,
        "{log}"
    );
    publish_head(&mut harness);
    green(&mut harness);
    harness
}

fn needs_fix(harness: ScenarioHarness) -> (ScenarioHarness, String) {
    let (harness, reviewed) = run(harness, "review_mr", 0);
    assert_eq!(reviewed.ledger_entries.len(), 2);
    let review = latest(&reviewed);
    assert_eq!(review["mode"], "review");
    assert_eq!(review["review_verdict"], "NEEDS_FIX");
    assert_eq!(review["human_required"], false);
    assert_eq!(
        review["review_blocking_findings"],
        json!([format!("implementation.txt:1: {FINDING}")])
    );
    assert_eq!(
        review["review_contract_version"],
        git_agent_harness::ledger::REVIEW_CONTRACT_VERSION
    );
    let generation = review["review_generation"].as_str().unwrap().to_string();
    (harness, generation)
}

fn approved() -> ScenarioHarness {
    let (harness, generation) = needs_fix(start());
    let (mut harness, repaired) = run(harness, "fix_mr", 0);
    assert_eq!(repaired.ledger_entries.len(), 3);
    let repair = latest(&repaired);
    assert_eq!(repair["dispatch_reason"], "post_review_repair");
    assert_eq!(repair["review_generation"], generation);
    assert_eq!(repair["push_succeeded"], true);
    assert_eq!(repair["validation_result"], "passed");
    let prompt = fs::read_to_string(
        std::path::Path::new(repair["session_dir"].as_str().unwrap()).join("attempt-1/task.md"),
    )
    .unwrap();
    assert!(prompt.contains(FINDING), "{prompt}");
    assert!(prompt.contains(&generation), "{prompt}");
    let sha = publish_head(&mut harness);
    assert_ne!(repair["review_source_sha"], sha);
    reviewer(&mut harness, "APPROVE");
    green(&mut harness);
    let (mut harness, reviewed) = run(harness, "review_mr", 0);
    assert_eq!(reviewed.ledger_entries.len(), 4);
    let review = latest(&reviewed);
    assert_eq!(review["review_verdict"], "APPROVE", "{review}");
    assert_eq!(review["review_confidence"], "high");
    assert_eq!(review["human_required"], false);
    assert_eq!(review["review_source_sha"], sha);
    assert_ne!(review["review_generation"], generation);
    let (next, ready) = run(harness, "mark_ready_for_review", 0);
    harness = next;
    assert_eq!(ready.ledger_entries, reviewed.ledger_entries);
    assert_eq!(
        harness.stateful_forge().state()["pulls"]["2"]["draft"],
        false
    );
    harness
}

#[test]
fn issue_to_needs_fix_to_repair_to_merge() {
    let (mut harness, merged) = run(approved(), "merge_mr", 0);
    assert_eq!(merged.ledger_entries.len(), 5);
    assert_eq!(latest(&merged)["mode"], "merge");
    assert_eq!(latest(&merged)["attempts_completed"], 1);
    assert!(latest(&merged)["error_summary"].is_null());
    let state = harness.stateful_forge().state();
    assert_eq!(state["pulls"]["2"]["state"], "closed");
    assert!(state["pulls"]["2"]["merged_at"].is_string());
    assert_eq!(state["branches"]["main"], state["pulls"]["2"]["merge_sha"]);
    let head = state["pulls"]["2"]["sha"].as_str().unwrap();
    assert!(harness.stateful_forge().calls().iter().any(|args| args
        .windows(2)
        .any(|pair| pair == ["--match-head-commit", head])));
}

#[test]
fn stale_review_generation_refuses_merge() {
    let mut harness = approved();
    // Change metadata only after the snapshot selected MergeMr. The branch
    // lookup belongs to runtime/merge.rs's executor, not snapshot collection.
    harness.stateful_forge().update(|s| {
        s["effects"] = json!([{
            "endpoint":"repos/owner/repo/pulls","method":"GET","at":1,
            "argv_contains":["state=all"],
            "patches":[{"path":["pulls","2","body"],"value":"Changed acceptance after approval"}]
        }])
    });
    let (mut harness, refused) = run(harness, "merge_mr", 0);
    assert_eq!(refused.ledger_entries.len(), 5);
    let entry = latest(&refused);
    assert_eq!(entry["mode"], "merge");
    assert_eq!(entry["failure_class"], "merge_failed");
    assert!(
        entry["error_summary"]
            .as_str()
            .unwrap()
            .contains("MR source or metadata changed after review"),
        "{entry}"
    );
    assert_eq!(
        harness.stateful_forge().state()["pulls"]["2"]["state"],
        "open"
    );
    assert!(!harness
        .stateful_forge()
        .calls()
        .iter()
        .any(|args| args.starts_with(&["gh".into(), "pr".into(), "merge".into()])));
}

#[test]
fn repair_retry_cap_requires_human() {
    let (harness, generation) = needs_fix(start());
    support::write_executable(
        &harness.bin_dir.join("agy"),
        "#!/bin/sh\necho repair failed >&2\nexit 23\n",
    );
    let (harness, first) = run(harness, "fix_mr", 1);
    assert_eq!(first.ledger_entries.len(), 3);
    let (harness, second) = run(harness, "fix_mr", 1);
    assert_eq!(second.ledger_entries.len(), 4);
    for repair in &second.ledger_entries[2..] {
        assert_eq!(repair["dispatch_reason"], "post_review_repair");
        assert_eq!(repair["review_generation"], generation);
        assert_eq!(repair["push_succeeded"], false);
        assert_eq!(repair["failure_class"], "backend_error");
    }
    let (mut harness, blocked) = run(harness, "human_required", 0);
    assert_eq!(blocked.ledger_entries, second.ledger_entries);
    assert!(
        blocked
            .events
            .iter()
            .any(|event| event["reason_code"] == "fix_retry_cap_exceeded"),
        "{:?}",
        blocked.events
    );
    assert_eq!(
        harness.stateful_forge().state()["pulls"]["2"]["state"],
        "open"
    );
}
