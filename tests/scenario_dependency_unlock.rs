//! Scenario 4 (#1520): a `Blocked by:` line or a native sub-issue keeps an
//! issue out of dispatch while its blocker is open, the next loop after the
//! blocker closes dispatches it, and a dependency cycle fails closed in status.

mod support;

use serde_json::{json, Value};
use support::scenario::{LoopResult, ScenarioHarness};

/// Local claims: an issue assigned to another login is managed by them and
/// never dispatched, so a blocker can stay open without being picked first.
fn harness() -> ScenarioHarness {
    let mut harness = ScenarioHarness::new("github").with_config_append(
        "validation_commands = [\"true\"]\n\
         [profiles.test.publishing]\ntrusted_issue_human_authors = [\"gah-bot\"]\n\
         allow_pull_request_creation = false\nallow_commit_message_generation = false\n\
         [profiles.test.routing]\n\
         improve_candidates = [{ backend = \"agy\", model = \"test-model\", priority = 100, included_in_quota = true }]\n",
    );
    support::write_executable(
        &harness.bin_dir.join("agy"),
        "#!/bin/sh\nset -eu\nif [ \"$1\" = --version ]; then echo 'test 1.0'; exit 0; fi\n\
         echo change >> implementation.txt\necho work complete\n",
    );
    let _ = harness.stateful_forge();
    harness
}

fn forge(harness: &mut ScenarioHarness, args: &[&str]) -> String {
    let output = harness
        .stateful_forge()
        .command("gh")
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn open_issue(harness: &mut ScenarioHarness, title: &str, body: &str) -> String {
    forge(
        harness,
        &[
            "api",
            "repos/owner/repo/issues",
            "--method",
            "POST",
            "-f",
            &format!("title={title}"),
            "-f",
            &format!("body={body}"),
        ],
    );
    let issues = harness.stateful_forge().state()["issues"].clone();
    let mut numbers: Vec<_> = issues.as_object().unwrap().keys().cloned().collect();
    numbers.sort_by_key(|n| n.parse::<u64>().unwrap());
    numbers.pop().unwrap()
}

fn managed_elsewhere(harness: &mut ScenarioHarness, number: &str) {
    harness.stateful_forge().update(|state| {
        state["issues"][number]["assignees"] = json!(["someone-else"]);
    });
}

fn close(harness: &mut ScenarioHarness, number: &str) {
    forge(harness, &["issue", "close", number]);
    assert_eq!(
        harness.stateful_forge().state()["issues"][number]["state"],
        "closed"
    );
}

fn blockers(harness: &mut ScenarioHarness) -> Vec<Value> {
    let status = harness.run_status_json().unwrap();
    status["dependency_blockers"].as_array().cloned().unwrap()
}

fn dispatched(result: &LoopResult) -> Vec<String> {
    result
        .ledger_entries
        .iter()
        .filter_map(|entry| entry["work_id"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn blocked_by_line_unlocks_when_the_blocker_closes() {
    let mut harness = harness();
    let a = open_issue(&mut harness, "Blocker", "Land the lease model first.");
    managed_elsewhere(&mut harness, &a);
    let b = open_issue(
        &mut harness,
        "Dependent",
        &format!("Validate leases.\n\nBlocked by: #{a}"),
    );

    let first = harness.run_one_loop().unwrap();
    assert!(dispatched(&first).is_empty(), "{first:?}");
    let blocked = blockers(&mut harness);
    assert_eq!(blocked.len(), 1, "{blocked:?}");
    assert_eq!(blocked[0]["work_id"], format!("#{b}"));
    assert_eq!(blocked[0]["reason_code"], "dependency_open");
    assert_eq!(blocked[0]["dependencies"][0]["identity"], format!("#{a}"));
    assert_eq!(blocked[0]["dependencies"][0]["normalized_state"], "open");

    close(&mut harness, &a);
    let second = harness.run_one_loop().unwrap();
    assert_eq!(dispatched(&second), vec![format!("#{b}")], "{second:?}");
    assert!(blockers(&mut harness).is_empty());
}

#[test]
fn dependency_cycle_fails_closed_and_is_visible_in_status() {
    let mut harness = harness();
    let c = open_issue(&mut harness, "First of a cycle", "Blocked by: #2");
    let d = open_issue(
        &mut harness,
        "Second of a cycle",
        &format!("Blocked by: #{c}"),
    );
    assert_eq!(d, "2");

    let result = harness.run_one_loop().unwrap();
    assert!(dispatched(&result).is_empty(), "{result:?}");
    let blocked = blockers(&mut harness);
    let mut ids: Vec<_> = blocked
        .iter()
        .map(|b| b["work_id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    assert_eq!(ids, vec![format!("#{c}"), format!("#{d}")], "{blocked:?}");
    assert!(
        blocked
            .iter()
            .all(|b| b["reason_code"] == "dependency_cycle"),
        "{blocked:?}"
    );
}

#[test]
fn native_sub_issue_blocks_the_parent_until_it_closes() {
    let mut harness = harness();
    let parent = open_issue(&mut harness, "Parent", "Tracked work.");
    let child = open_issue(&mut harness, "Child", "A sub-task.");
    managed_elsewhere(&mut harness, &child);
    let child_id = harness.stateful_forge().state()["issues"][&child]["id"]
        .as_i64()
        .unwrap();
    forge(
        &mut harness,
        &[
            "api",
            &format!("repos/owner/repo/issues/{parent}/sub_issues"),
            "--method",
            "POST",
            "-F",
            &format!("sub_issue_id={child_id}"),
        ],
    );

    let first = harness.run_one_loop().unwrap();
    assert!(dispatched(&first).is_empty(), "{first:?}");
    let blocked = blockers(&mut harness);
    assert_eq!(blocked.len(), 1, "{blocked:?}");
    assert_eq!(blocked[0]["work_id"], format!("#{parent}"));
    assert_eq!(blocked[0]["reason_code"], "dependency_open");
    assert_eq!(
        blocked[0]["dependencies"][0]["identity"],
        format!("#{child}")
    );

    close(&mut harness, &child);
    let second = harness.run_one_loop().unwrap();
    assert_eq!(
        dispatched(&second),
        vec![format!("#{parent}")],
        "{second:?}"
    );
    assert!(blockers(&mut harness).is_empty());
}
