//! Scenario 8 (#1530): a rate-limited route is marked unavailable and the next
//! dispatch uses the fallback; advancing the injected clock past the cooldown
//! retries the primary and clears it on success; with every candidate out the
//! loop holds instead of spinning and status says why.

mod support;

use serde_json::Value;
use std::fs;
use support::scenario::ScenarioHarness;
use time::{Duration, OffsetDateTime};

const RATE_LIMITED: &str = "#!/bin/sh\nlog=\"${XDG_DATA_HOME:-$HOME/.local/share}/opencode/log\"\nmkdir -p \"$log\"\n\
    printf '%s\\n' 'timestamp=now level=ERROR message=\"AI_APICallError: Rate limit exceeded. Please try again later.\"' >> \"$log/opencode.log\"\nexit 0\n";
/// Codex reports capacity in its own output; replay the recorded fixture.
fn codex_rate_limited() -> String {
    format!(
        "#!/bin/sh\ncat '{}'\nexit 0\n",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/quota-logs/codex_selected_model_at_capacity.jsonl"
        )
    )
}

const WORKING: &str = "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'test 1.0'; exit 0; fi\nprintf 'edit\\n' >> implementation.txt\necho work complete\n";

const BASE: &str = "2030-01-01T00:00:00Z";

fn base() -> OffsetDateTime {
    OffsetDateTime::parse(BASE, &time::format_description::well_known::Rfc3339).unwrap()
}

fn harness() -> ScenarioHarness {
    let mut harness = ScenarioHarness::new("github").with_config_append(
        "validation_commands = [\"true\"]\n\
         [profiles.test.publishing]\nallow_pull_request_creation = false\nallow_commit_message_generation = false\n\
         [profiles.test.routing]\n\
         improve_candidates = [{ backend = \"opencode\", model = \"opencode/primary\", priority = 100, included_in_quota = true }, { backend = \"codex\", model = \"fallback\", priority = 50, included_in_quota = true }]\n",
    );
    harness.set_now(base());
    support::write_executable(&harness.bin_dir.join("opencode"), RATE_LIMITED);
    support::write_executable(&harness.bin_dir.join("codex"), WORKING);
    harness
}

fn dispatch(
    harness: &mut ScenarioHarness,
    target: &str,
    retries: &str,
) -> support::scenario::CommandResult {
    harness
        .run_dispatch(&[
            "--mode",
            "fix",
            "--skip-validation-gate",
            "--retries",
            retries,
            "--target",
            target,
        ])
        .unwrap()
}

fn ledger(harness: &ScenarioHarness) -> Vec<Value> {
    // One line per dispatch; the newest is last.
    fs::read_to_string(&harness.ledger_path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn attempts(entry: &Value) -> Vec<(String, String)> {
    entry["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|attempt| {
            (
                attempt["backend"].as_str().unwrap().to_string(),
                attempt["validation_result"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
            )
        })
        .collect()
}

fn availability(harness: &mut ScenarioHarness) -> Vec<Value> {
    let status = harness.run_status_json().unwrap();
    status["availability"].as_array().cloned().unwrap()
}

#[test]
fn rate_limited_primary_is_marked_unavailable_and_the_next_dispatch_uses_the_fallback() {
    let mut harness = harness();
    let first = dispatch(&mut harness, "first change", "1");
    assert!(first.exit_code == Some(0), "{first:?}");
    let entries = ledger(&harness);
    assert_eq!(
        attempts(&entries[0]),
        vec![
            ("opencode".into(), "not_run_backend_unavailable".into()),
            ("codex".into(), "passed".into())
        ],
        "{entries:?}"
    );
    let scopes = availability(&mut harness);
    assert!(
        scopes
            .iter()
            .any(|scope| scope["backend"] == "opencode" && scope["reason"] == "rate_limited"),
        "{scopes:?}"
    );

    // The primary is skipped outright now: no attempt on it at all.
    let second = dispatch(&mut harness, "second change", "0");
    assert!(second.exit_code == Some(0), "{second:?}");
    eprintln!(
        "LEDGER AFTER SECOND:\n{}",
        fs::read_to_string(&harness.ledger_path).unwrap_or_default()
    );
    eprintln!(
        "SECOND STDOUT:\n{}\nSECOND STDERR:\n{}",
        second.stdout, second.stderr
    );
    let entries = ledger(&harness);
    assert_eq!(
        attempts(entries.last().unwrap()),
        vec![("codex".into(), "passed".into())],
        "{entries:?}"
    );
}

#[test]
fn advancing_the_clock_past_the_cooldown_retries_the_primary_and_clears_it_on_success() {
    let mut harness = harness();
    dispatch(&mut harness, "first change", "1");
    assert!(availability(&mut harness)
        .iter()
        .any(|scope| scope["backend"] == "opencode"));
    let start = OffsetDateTime::parse(
        "2030-01-01T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let _ = start;

    // Still inside the cooldown: the primary stays skipped.
    let now = base();
    harness.set_now(now + Duration::seconds(30));
    dispatch(&mut harness, "second change", "0");
    assert_eq!(
        attempts(ledger(&harness).last().unwrap()),
        vec![("codex".into(), "passed".into())]
    );

    // Past the conservative one-minute rate-limit cooldown the primary is eligible again; it now works.
    support::write_executable(&harness.bin_dir.join("opencode"), WORKING);
    harness.set_now(now + Duration::minutes(2));
    let third = dispatch(&mut harness, "third change", "0");
    assert!(third.exit_code == Some(0), "{third:?}");
    assert_eq!(
        attempts(ledger(&harness).last().unwrap()),
        vec![("opencode".into(), "passed".into())]
    );
    let scopes = availability(&mut harness);
    assert!(
        !scopes
            .iter()
            .any(|scope| scope["backend"] == "opencode" && scope["reason"] == "rate_limited"),
        "{scopes:?}"
    );
}

#[test]
fn every_candidate_exhausted_defers_instead_of_spinning_and_status_reports_the_block() {
    let mut harness = harness();
    support::write_executable(&harness.bin_dir.join("codex"), &codex_rate_limited());
    // Exit status is not the contract here: the attempts, the recorded
    // availability, and the absence of a retry storm are.
    let _result = dispatch(&mut harness, "no route left", "1");
    let entries = ledger(&harness);
    let latest = entries.last().unwrap();
    // The primary is rate-limited and skipped; the fallback's "model at
    // capacity" on the final attempt is a capacity deferral, not a retry.
    assert_eq!(
        attempts(latest),
        vec![("opencode".into(), "not_run_backend_unavailable".into())],
        "{entries:?}"
    );
    assert_eq!(
        latest["validation_result"], "deferred_capacity",
        "{entries:?}"
    );
    let scopes = availability(&mut harness);
    assert!(
        scopes
            .iter()
            .any(|scope| scope["backend"] == "opencode" && scope["reason"] == "rate_limited"),
        "{scopes:?}"
    );

    // Dispatching again while nothing is eligible defers again at once: at
    // most one routing attempt, no retry storm.
    let held = dispatch(&mut harness, "still nothing", "1");
    let entries = ledger(&harness);
    let latest = entries.last().unwrap();
    assert!(attempts(latest).len() <= 1, "{entries:?} {held:?}");
    assert_eq!(
        latest["validation_result"], "deferred_capacity",
        "{entries:?}"
    );
}
