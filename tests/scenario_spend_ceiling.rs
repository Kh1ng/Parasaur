//! #1537: real CLI intake stops at recorded spend, resumes on UTC rollover,
//! and emits one pause notice without replacing durable execution history.
mod support;
use std::{fs, process::Command};
use support::{fake_ledger::TestLedger, scenario::ScenarioHarness};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

fn scenario(window: &str) {
    let mut harness = ScenarioHarness::new("github").github_scenario("empty");
    let notice = harness.artifacts_dir.join("budget-notices.txt");
    let marker = harness.artifacts_dir.join("backend-starts.txt");
    harness = harness.with_config_append(&format!(
        "validation_commands = [\"true\"]\nnotify_command = \"cat >> {}\"\n\
        [profiles.test.publishing]\nallow_pull_request_creation = false\nallow_commit_message_generation = false\n\
        [profiles.test.routing]\nimprove_candidates = [{{backend = \"opencode\", model = \"opencode/test\", priority = 100, included_in_quota = false}}]\n\
        [profiles.test.routing.spend_budget]\n{window}_usd = 1.0\n", notice.display()));
    harness.set_now(OffsetDateTime::parse("2030-01-31T12:00:00Z", &Rfc3339).unwrap());
    support::write_executable(&harness.bin_dir.join("opencode"), &format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'test 1.0'; exit 0; fi\necho START >> '{}'\necho change >> implementation.txt\necho 'actual_cost_usd: 1.0'\n", marker.display()));
    let tickets = harness.local_repo_dir.join("docs/tickets");
    fs::create_dir_all(&tickets).unwrap();
    for number in [1, 2] {
        fs::write(
            tickets.join(format!("TICKET-{number:03}-budget.md")),
            format!("# TICKET-{number:03}: fixture work\n\nImplement a small local change.\n"),
        )
        .unwrap();
    }
    for args in [vec!["add", "."], vec!["commit", "-m", "fixture tickets"]] {
        assert!(Command::new("git")
            .args(args)
            .current_dir(&harness.local_repo_dir)
            .output()
            .unwrap()
            .status
            .success());
    }
    let first = harness.run_one_loop().unwrap();
    assert_eq!(first.exit_code, Some(0), "{first:?}");
    assert_eq!(
        fs::read_to_string(&marker).unwrap().lines().count(),
        1,
        "{first:?}"
    );
    let history = TestLedger::read_from(&harness.ledger_path).unwrap();
    assert!(!history.is_empty());
    assert!(
        history
            .iter()
            .any(|entry| entry["usage"]["actual_cost_usd"] == 1.0),
        "fixture must record an actual cost before testing the ceiling"
    );
    let mut retained = TestLedger::new();
    for entry in &history {
        retained = retained.with_entry(entry.clone());
    }
    // The harness materializes its configured input for each child. Feed back
    // the real first dispatch's complete history, rather than clearing it.
    harness = harness.with_ledger(retained);
    let held = harness.run_one_loop().unwrap();
    assert_eq!(held.exit_code, Some(0), "{held:?}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&held.stdout).unwrap()["action"]["type"],
        "WaitUntil"
    );
    assert!(held.stdout.contains("spend_budget"));
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);
    assert_eq!(
        TestLedger::read_from(&harness.ledger_path).unwrap(),
        history
    );
    let repeated = harness.run_one_loop().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&repeated.stdout).unwrap()["action"]["type"],
        "WaitUntil"
    );
    assert_eq!(fs::read_to_string(&notice).unwrap().lines().count(), 1);
    assert_eq!(
        repeated
            .events
            .iter()
            .filter(|event| event["event_type"] == "spend_budget_held")
            .count(),
        1
    );
    assert!(!repeated
        .events
        .iter()
        .any(|event| event["event_type"] == "terminal_failure"));
    let status = harness.run_status_json().unwrap();
    assert_eq!(status["spend_budget"]["intake_held"], true);
    assert_eq!(status["spend_budget"]["windows"][0]["reported_usd"], 1.0);
    assert_eq!(
        status["spend_budget"]["windows"][0]["unmeasured_observations"],
        0
    );
    // Direct CLI execution shares the guard, even outside the loop.
    let refused = harness
        .run_dispatch(&[
            "--mode",
            "fix",
            "--target",
            "another change",
            "--skip-validation-gate",
        ])
        .unwrap();
    assert_ne!(refused.exit_code, Some(0));
    assert!(refused.stderr.contains("spend budget"), "{refused:?}");
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);
    harness.set_now(OffsetDateTime::parse("2030-02-01T00:00:00Z", &Rfc3339).unwrap());
    let resumed = harness.run_one_loop().unwrap();
    assert_eq!(resumed.exit_code, Some(0), "{resumed:?}");
    assert_eq!(
        fs::read_to_string(&marker).unwrap().lines().count(),
        2,
        "{resumed:?}"
    );
}

#[test]
fn daily_ceiling_pauses_intake_and_resumes_next_utc_day() {
    scenario("daily");
}

#[test]
fn monthly_ceiling_pauses_intake_and_resumes_next_utc_month() {
    scenario("monthly");
}
