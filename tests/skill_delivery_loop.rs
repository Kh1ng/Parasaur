//! Bound content must survive the real loop -> dispatch -> worker boundary.

mod support;

use std::fs;
use std::process::Command;
use support::scenario::ScenarioHarness;

fn assert_skill_delivery(backend: &str) {
    let mut harness = ScenarioHarness::new("github").github_scenario("empty");
    let root = harness.artifacts_dir.parent().unwrap();
    let bank = root.join("home/.config/gah/skills.json");
    fs::create_dir_all(bank.parent().unwrap()).unwrap();
    fs::write(
        &bank,
        serde_json::to_vec(&serde_json::json!({
            "skills": [{
                "id": "loop-skill", "version": "2.1.0",
                "content": "  Follow the bound skill sentinel instructions.\n",
            }],
            "bindings": {"loop-skill": [format!("backend:{backend}")]},
        }))
        .unwrap(),
    )
    .unwrap();
    let marker = harness.artifacts_dir.join("worker-argv");
    support::write_executable(
        &harness.bin_dir.join(backend),
        &format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'test 1.0'; exit 0; fi\n\
             printf '%s\\n' \"$@\" > '{}'\necho 'deliberate worker failure'\nexit 1\n",
            marker.display()
        ),
    );
    let availability = root.join("xdg-state/gah/availability.json");
    fs::create_dir_all(availability.parent().unwrap()).unwrap();
    fs::write(
        availability,
        serde_json::to_vec(&serde_json::json!({
            "version": 2,
            "records": [{
                "backend": backend, "status": "available", "reason": "unknown",
                "observed_at": chrono::Utc::now().to_rfc3339(), "source": "manual",
            }],
        }))
        .unwrap(),
    )
    .unwrap();
    let config = format!(
        "validation_commands = [\"true\"]\n{backend}_path = {:?}\n\
         [profiles.test.routing]\n\
         improve_candidates = [{{ backend = {backend:?}, model = \"test-model\", priority = 100, included_in_quota = true }}]\n",
        harness.bin_dir.join(backend).to_str().unwrap()
    );
    harness = harness.with_config_append(&config);
    let ticket = "docs/tickets/TICKET-243-skill-delivery.md";
    fs::create_dir_all(harness.local_repo_dir.join("docs/tickets")).unwrap();
    fs::write(
        harness.local_repo_dir.join(ticket),
        format!("# TICKET-243: Deliver bound skills\n\nGoal: verify worker skill delivery.\n\nRecommended backend: {backend}\n"),
    )
    .unwrap();
    for args in [
        vec!["add", ticket],
        vec!["commit", "-m", "add skill delivery fixture"],
        vec!["push", "-q", "origin", "main"],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(&harness.local_repo_dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    let result = harness.run_one_loop().unwrap();
    assert!(marker.exists(), "{backend}: {result:?}");
    let argv = fs::read_to_string(marker).unwrap();
    assert!(
        argv.contains("# Bound skills\nThese are trusted instructions from the central GAH skill bank.\n\n## loop-skill@2.1.0\nFollow the bound skill sentinel instructions.\n\n# Current request\n"),
        "{backend} dropped bound content: {argv}"
    );
    assert!(argv.contains("TICKET-243-skill-delivery.md"), "{argv}");
    if matches!(backend, "hermes" | "openhands") {
        assert!(argv.contains("\n--skills\nloop-skill\n"), "{argv}");
    } else {
        assert!(!argv.contains("\n--skills\n"), "{argv}");
    }
    assert!(
        result
            .ledger_entries
            .iter()
            .any(|entry| entry["backend"] == backend),
        "{result:?}"
    );
}

#[test]
fn codex_loop_delivers_bound_skill_content() {
    assert_skill_delivery("codex");
}

#[test]
fn claude_loop_delivers_bound_skill_content() {
    assert_skill_delivery("claude");
}

#[test]
fn opencode_loop_delivers_bound_skill_content() {
    assert_skill_delivery("opencode");
}

#[test]
fn hermes_loop_delivers_bound_skill_content_and_native_ids() {
    assert_skill_delivery("hermes");
}

#[test]
fn openhands_loop_delivers_bound_skill_content_and_native_ids() {
    assert_skill_delivery("openhands");
}
