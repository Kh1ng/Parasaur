//! Skill-bank outages must only block runners that require skills.

mod support;

use std::fs;
use std::net::TcpListener;
use support::scenario::{read_jsonl_lines, ScenarioHarness};

fn dispatch_with_central_down(backend: &str, policy: Option<&str>, proceeds: bool) {
    // Use a closed local port so curl reports a connection failure without a timeout.
    let central = TcpListener::bind("127.0.0.1:0").unwrap();
    let central_url = format!("http://{}", central.local_addr().unwrap());
    drop(central);
    let mut config = String::new();
    if let Some(policy) = policy {
        config.push_str(&format!("skill_resolve_failure_policy = {policy:?}\n"));
    }
    config.push_str(
        "[profiles.test.publishing]\nallow_pull_request_creation = false\n\
         allow_commit_message_generation = false\n",
    );
    let mut harness = ScenarioHarness::new("github")
        .with_defaults_append(&format!("registry_central_url = {central_url:?}"))
        .with_config_append(&config);
    let marker = harness.artifacts_dir.join("worker-started");
    support::write_executable(
        &harness.bin_dir.join(backend),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'test 1.0'; exit 0; fi\n\
             printf '%s\\n' \"$@\" > '{}'\necho 'agent change' >> README.md\necho 'work complete'\n",
            marker.display()
        ),
    );
    let result = harness
        .run_dispatch(&[
            "--mode",
            "fix",
            "--backend",
            backend,
            "--model",
            "test-model",
            "--target",
            "exercise unavailable skill bank",
        ])
        .unwrap();
    assert_eq!(marker.exists(), proceeds, "{backend}: {result:?}");
    assert_eq!(
        result.exit_code == Some(0),
        proceeds,
        "{backend}: {result:?}"
    );
    assert!(
        result.stderr.contains("skill resolution failed"),
        "{result:?}"
    );

    let entries = read_jsonl_lines(&harness.ledger_path).unwrap();
    let entry = entries.last().unwrap();
    let session = entry["session_dir"].as_str().unwrap();
    let record: serde_json::Value = serde_json::from_slice(
        &fs::read(std::path::Path::new(session).join("attempt-1/skill-resolution-failure.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(record["backend"], backend);
    assert_eq!(record["policy"], policy.unwrap_or("fail_closed"));
    assert_eq!(
        record["action"],
        if proceeds {
            "proceed_without_skills"
        } else {
            "fail_closed"
        }
    );
    assert!(record["error"]
        .as_str()
        .unwrap()
        .contains("no usable last-known-good skill cache"));
    if proceeds {
        assert_eq!(entry["backend_exit_code"], 0, "{entry}");
        assert!(!fs::read_to_string(marker).unwrap().contains("--skills"));
    }
}

#[test]
fn runners_without_skills_dispatch_when_central_is_down_and_cache_is_empty() {
    for backend in ["codex", "claude", "opencode", "vibe", "agy"] {
        dispatch_with_central_down(backend, None, true);
    }
}

#[test]
fn skill_runners_fail_closed_by_default_and_record_the_failure() {
    for backend in ["hermes", "openhands"] {
        dispatch_with_central_down(backend, None, false);
    }
}

#[test]
fn skill_runners_can_proceed_without_skills_and_record_the_policy() {
    for backend in ["hermes", "openhands"] {
        dispatch_with_central_down(backend, Some("proceed_without_skills"), true);
    }
}
