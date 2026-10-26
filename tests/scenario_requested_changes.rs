//! Human review imports exercised through the real controller and persistent forge.

mod support;

use serde_json::{json, Value};
use std::fs;
use std::process::Command;
use support::fake_ledger::TestLedger;
use support::scenario::{LoopResult, ScenarioHarness};
use support::{FakeBackend, Scenario};

const REVIEW_ID: u64 = 1524;
const REVIEW_BODY: &str = "Validate both lease expiry and publication ownership";
const COMMENTS: [(&str, u64, &str); 2] = [
    (
        "src/lease.rs",
        12,
        "Reject the expired lease before starting publication",
    ),
    (
        "src/publish.rs",
        34,
        "Recheck the claim owner before pushing the result",
    ),
];

fn harness() -> ScenarioHarness {
    let mut harness = ScenarioHarness::new("github").with_config_append(
        "validation_commands = [\"true\"]\n\
         [profiles.test.publishing]\ntrusted_issue_human_authors = [\"gah-bot\", \"maintainer\"]\n\
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
    harness.stateful_forge();
    support::write_executable(
        &harness.bin_dir.join("agy"),
        "#!/bin/sh\nset -eu\nif [ \"$1\" = --version ]; then echo 'test 1.0'; exit 0; fi\n\
         echo change >> implementation.txt\necho work complete\n",
    );
    reviewer(&mut harness);
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

fn reviewer(harness: &mut ScenarioHarness) {
    let reviewer = FakeBackend::new(&harness.artifacts_dir, "codex");
    reviewer.install(Scenario::success().with_stdout(json!({
        "verdict":"APPROVE","confidence":"high","human_required":false,
        "actionable_findings":[],"blocking_findings":[],
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
    publish_head(&mut harness);
    green(&mut harness);
    harness
}

// REST-shaped reviews and inline comments are read by the production importer.
fn request_changes(harness: &mut ScenarioHarness, login: &str) {
    harness.stateful_forge().update(|state| {
        let sha = state["pulls"]["2"]["sha"].clone();
        let now = state["now"].clone();
        let pull = &mut state["pulls"]["2"];
        pull["reviews"] = json!([{
            "id": REVIEW_ID, "user": {"login": login},
            "state": "CHANGES_REQUESTED", "body": REVIEW_BODY,
            "commit_id": sha, "submitted_at": now,
        }]);
        pull["review_comments"] = json!(COMMENTS
            .iter()
            .enumerate()
            .map(|(index, (path, line, body))| {
                json!({
                    "id": 1600 + index, "pull_request_review_id": REVIEW_ID,
                    "user": {"login": login}, "path": path, "line": line,
                    "side": "RIGHT", "body": body, "commit_id": sha, "created_at": now,
                })
            })
            .collect::<Vec<_>>());
        // A comment from another review must not leak into this repair.
        pull["review_comments"].as_array_mut().unwrap().push(json!({
            "id": 1700, "pull_request_review_id": REVIEW_ID + 1,
            "user": {"login": login}, "path": "unrelated.rs", "line": 99,
            "body": "Unrelated review finding", "created_at": now,
        }));
    });
}

fn fix_dispatches(result: &LoopResult) -> usize {
    result
        .events
        .iter()
        .filter(|event| {
            event["event_type"] == "action_decided"
                && event["details"].as_str().unwrap().split(':').next() == Some("fix_mr")
        })
        .count()
}

fn imports(result: &LoopResult) -> Vec<&Value> {
    result
        .ledger_entries
        .iter()
        .filter(|entry| entry["dispatch_reason"] == format!("requested_changes:{REVIEW_ID}"))
        .collect()
}

#[test]
fn trusted_changes_requested_dispatches_repair_with_inline_comments() {
    let mut harness = start();
    request_changes(&mut harness, "maintainer");
    let (_harness, repaired) = run(harness, "fix_mr", 0);
    assert_eq!(fix_dispatches(&repaired), 1);
    assert_eq!(repaired.ledger_entries.len(), 3);
    let imported = imports(&repaired);
    assert_eq!(imported.len(), 1);
    let review = imported[0];
    assert_eq!(review["mode"], "review");
    assert_eq!(review["reviewer_backend"], "human:maintainer");
    assert_eq!(review["review_verdict"], "NEEDS_FIX");
    assert_eq!(
        review["review_contract_version"],
        git_agent_harness::ledger::REVIEW_CONTRACT_VERSION
    );
    assert!(review["review_metadata_fingerprint"].is_string());
    let repair = latest(&repaired);
    assert_eq!(repair["mode"], "fix");
    assert_eq!(repair["dispatch_reason"], "post_review_repair");
    assert_eq!(repair["review_generation"], review["review_generation"]);
    assert_eq!(repair["push_succeeded"], true);
    assert_eq!(repair["validation_result"], "passed");
    let prompt = fs::read_to_string(
        std::path::Path::new(repair["session_dir"].as_str().unwrap()).join("attempt-1/task.md"),
    )
    .unwrap();
    assert!(prompt.contains(REVIEW_BODY), "{prompt}");
    assert!(
        prompt.contains(review["review_generation"].as_str().unwrap()),
        "{prompt}"
    );
    for (path, line, body) in COMMENTS {
        let finding = format!("{path}:{line}: {body}");
        assert!(review["review_blocking_findings"]
            .as_array()
            .unwrap()
            .contains(&json!(finding)));
        assert!(prompt.contains(&finding), "{prompt}");
    }
    assert!(!prompt.contains("Unrelated review finding"), "{prompt}");
}

#[test]
fn untrusted_changes_requested_does_not_dispatch_repair() {
    let mut harness = start();
    request_changes(&mut harness, "outsider");
    let (harness, reviewed) = run(harness, "review_mr", 0);
    assert_eq!(latest(&reviewed)["review_verdict"], "APPROVE");
    assert!(imports(&reviewed).is_empty());
    assert_eq!(fix_dispatches(&reviewed), 0);
    let (_, deferred) = run(harness, "wait_until", 0);
    assert_eq!(fix_dispatches(&deferred), 0);
    assert_eq!(deferred.ledger_entries, reviewed.ledger_entries);
    assert!(
        deferred.events.iter().any(|event| {
            event["event_type"] == "action_decided"
                && event["details"].as_str().is_some_and(|details| {
                    details.contains("current-head review veto from outsider")
                })
        }),
        "{:?}",
        deferred.events
    );
    assert!(!deferred
        .ledger_entries
        .iter()
        .any(|entry| entry["dispatch_reason"] == "post_review_repair"));
}

#[test]
fn identical_changes_requested_does_not_dispatch_second_repair() {
    let mut harness = start();
    request_changes(&mut harness, "maintainer");
    let (mut harness, repaired) = run(harness, "fix_mr", 0);
    assert_eq!(fix_dispatches(&repaired), 1);
    assert_eq!(imports(&repaired).len(), 1);
    assert_eq!(latest(&repaired)["push_succeeded"], true);
    let review = imports(&repaired)[0];
    // Reflect the real push, as in scenario_lifecycle; leave the human review intact.
    let sha = publish_head(&mut harness);
    assert_ne!(review["review_source_sha"], sha);
    let pull = harness.stateful_forge().state()["pulls"]["2"].clone();
    let (harness, reviewed) = run(harness, "review_mr", 0);
    assert_ne!(
        latest(&reviewed)["review_generation"],
        review["review_generation"]
    );
    assert_eq!(latest(&reviewed)["review_source_sha"], sha);
    // No fixture changes: pending CI now reaches the profile's human merge policy.
    let (mut harness, repeated) = run(harness, "human_required", 0);
    assert_eq!(fix_dispatches(&repeated), 1);
    assert_eq!(imports(&repeated), imports(&repaired));
    assert_eq!(repeated.ledger_entries, reviewed.ledger_entries);
    assert_eq!(
        repeated
            .ledger_entries
            .iter()
            .filter(|entry| { entry["dispatch_reason"] == "post_review_repair" })
            .count(),
        1
    );
    let current = harness.stateful_forge().state()["pulls"]["2"].clone();
    assert_eq!(current["reviews"], pull["reviews"]);
    assert_eq!(current["review_comments"], pull["review_comments"]);
}
