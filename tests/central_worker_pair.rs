//! Real worker CLI dispatches against an inspectable, in-process central API.

mod support;

use serde_json::{json, Value};
use std::{fs, process::Command};
use support::scenario::{central::FakeCentral, ScenarioHarness};

const TICKET: &str = "docs/tickets/TICKET-1516-central-worker.md";

fn resolution() -> Value {
    json!({
        "profile":"test", "backend":"codex", "instance":"worker-codex", "source":"profile",
        "skills":[{"id":"central-skill", "version":"1.2.3", "backends":[], "content":"Follow the central worker sentinel instructions."}]
    })
}

fn worker(central: &FakeCentral) -> ScenarioHarness {
    let mut harness = ScenarioHarness::new("github")
        .github_scenario("empty")
        .with_central(central);
    let root = harness.artifacts_dir.parent().unwrap();
    let availability = root.join("xdg-state/gah/availability.json");
    fs::create_dir_all(availability.parent().unwrap()).unwrap();
    fs::write(
        availability,
        json!({"version":2,"records":[{
            "backend":"codex", "backend_instance":"worker-codex", "status":"available",
            "reason":"unknown", "observed_at":chrono::Utc::now().to_rfc3339(), "source":"manual"
        }]})
        .to_string(),
    )
    .unwrap();
    // This timestamp is an inventory observation, not the resolution-cache timestamp.
    let observed = chrono::Utc::now() - chrono::Duration::minutes(20);
    fs::write(
        root.join("xdg-state/gah/skill_inventory.jsonl"),
        json!({
            "profile":"test", "backend":"codex", "backend_instance":"worker-codex",
            "observed_skill_ids":["central-skill"], "observed_at":observed.to_rfc3339()
        })
        .to_string()
            + "\n",
    )
    .unwrap();
    let marker = harness.artifacts_dir.join("worker-argv");
    support::write_executable(&harness.bin_dir.join("codex"), &format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'test 1.0'; exit 0; fi\nprintf '%s\\n' \"$@\" > '{}'\necho 'deliberate worker failure'\nexit 1\n", marker.display()
    ));
    let config = format!(
        "validation_commands = [\"true\"]\ncodex_path = {:?}\n\
         [profiles.test.publishing]\nallow_pull_request_creation = false\nallow_commit_message_generation = false\n\
         [profiles.test.routing]\nimprove_candidates = [{{ backend = \"codex\", instance = \"worker-codex\", model = \"test-model\", priority = 100, included_in_quota = true }}]\n\
         [profiles.test.routing.backend_instances.worker-codex]\nrunner_kind = \"codex\"\nexecutable = {:?}\n",
        harness.bin_dir.join("codex"), harness.bin_dir.join("codex")
    );
    harness = harness.with_config_append(&config);
    fs::create_dir_all(harness.local_repo_dir.join("docs/tickets")).unwrap();
    fs::write(harness.local_repo_dir.join(TICKET),
        "# TICKET-1516: Exercise a central worker pair\n\nGoal: verify central skill dispatch.\n\nRecommended backend: codex\n").unwrap();
    for args in [
        vec!["add", TICKET],
        vec!["commit", "-m", "add ticket"],
        vec!["push", "-q", "origin", "main"],
    ] {
        let output = Command::new("git")
            .args(args)
            .current_dir(&harness.local_repo_dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    central.update(|state| {
        state["resolutions"]["test/codex/worker-codex"] = resolution();
        state["quotas"]["test"] = json!({"profile":"test", "since":"7d", "nodes":[{
            "nodeId":"worker-a", "displayName":"Worker A", "state":"unavailable", "quota":null,
            "error":"fixture: worker quota unavailable"
        }]});
    });
    harness
}

fn assert_skill_argv(harness: &ScenarioHarness) {
    let argv = fs::read_to_string(harness.artifacts_dir.join("worker-argv")).unwrap();
    assert!(
        argv.contains("## central-skill@1.2.3\nFollow the central worker sentinel instructions."),
        "{argv}"
    );
}

fn assert_inventory_status(status: &Value) {
    let inventory = status["skill_inventory"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["backend_instance"] == "worker-codex")
        .unwrap();
    assert_eq!(inventory["bound_skill_ids"], json!(["central-skill"]));
    assert!(
        inventory["observation_age_seconds"].as_i64().unwrap() >= 1200,
        "{inventory}"
    );
    assert_eq!(inventory["observation_stale"], true);
}

#[test]
fn worker_loop_claims_resolves_and_delivers_central_skill() {
    let central = FakeCentral::new();
    let mut harness = worker(&central);
    let central_status = harness.central_status();
    assert_eq!(central_status["node"]["role"], "central");
    let result = harness.run_one_loop().unwrap();
    assert!(
        harness.artifacts_dir.join("worker-argv").exists(),
        "{result:?}"
    );
    assert_skill_argv(&harness);
    assert!(
        result
            .ledger_entries
            .iter()
            .any(|entry| entry["backend"] == "codex" && entry["backend_exit_code"] == 1),
        "{result:?}"
    );
    let state = central.state();
    let calls = state["calls"].as_array().unwrap();
    let acquire = calls
        .iter()
        .position(|call| call["target"] == "/api/claims/acquire")
        .unwrap();
    let resolve = calls
        .iter()
        .position(|call| {
            call["target"]
                .as_str()
                .unwrap()
                .starts_with("/api/skills/resolve?")
        })
        .unwrap();
    let release = calls
        .iter()
        .position(|call| call["target"] == "/api/claims/release")
        .unwrap();
    assert!(acquire < resolve && resolve < release, "{calls:?}");
    assert_eq!(calls[acquire]["body"]["work_id"], "#1516");
    assert_eq!(calls[acquire]["body"]["profile"], "test");
    let node = calls[acquire]["body"]["node_id"].as_str().unwrap();
    assert!(
        state["nodes"].get(node).is_some(),
        "worker must register before claiming"
    );
    assert_eq!(state["claims"], json!({}), "released on backend failure");
    let before_status = calls.len();
    let status = harness.run_status_json().unwrap();
    assert_eq!(status["node"]["role"], "worker");
    assert_inventory_status(&status);
    assert_eq!(
        central.state()["calls"].as_array().unwrap().len(),
        before_status,
        "status reads the warm cache without HTTP"
    );

    // The quota relay is a server/dashboard client, not a Rust worker-loop client.
    let output = Command::new("curl")
        .args([
            "-q",
            "-fsS",
            "--max-time",
            "2",
            &format!("{}/api/registry/quota?profile=test&since=7d", central.url),
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        state["quotas"]["test"]
    );
    assert_eq!(
        central.state()["calls"].as_array().unwrap().last().unwrap()["target"],
        "/api/registry/quota?profile=test&since=7d"
    );
}

#[test]
fn outage_refuses_central_claim_then_explicit_local_mode_dispatches_warmed_skills() {
    let mut central = FakeCentral::new();
    let mut harness = worker(&central);
    let warm = harness.run_one_loop().unwrap();
    assert!(
        harness.artifacts_dir.join("worker-argv").exists(),
        "{warm:?}"
    );
    assert_skill_argv(&harness);
    let marker = harness.artifacts_dir.join("worker-argv");
    fs::remove_file(&marker).unwrap();
    central.shutdown();

    let refused = harness.run_one_loop().unwrap();
    assert!(
        !marker.exists(),
        "central outage must not start a worker: {refused:?}"
    );
    assert!(
        refused.ledger_entries.is_empty(),
        "no attempt/local ledger claim on refusal: {refused:?}"
    );
    assert!(
        refused.events.iter().any(|event| {
            event["event_type"] == "dispatch_finished"
                && event["details"]
                    .as_str()
                    .is_some_and(|details| details.contains("curl") && details.contains("failed"))
        }),
        "refusal must be durable in controller events: {refused:?}"
    );
    let status = harness.run_status_json().unwrap();
    assert_eq!(status["node"]["role"], "worker");
    assert_inventory_status(&status);

    // Direct free-text dispatch has no work id to arbitrate. It reaches the real
    // remote resolver while the listener is closed, proving warm-cache fallback.
    let cached = harness
        .run_dispatch(&[
            "--mode",
            "fix",
            "--backend",
            "codex",
            "--skip-validation-gate",
            "--model",
            "test-model",
            "--retries",
            "0",
            "--target",
            "exercise cached skill resolution",
        ])
        .unwrap();
    assert!(
        cached.stderr.contains("using last-known-good cache"),
        "{cached:?}"
    );
    assert_skill_argv(&harness);
    fs::remove_file(&marker).unwrap();

    // Explicit operator action, not automatic fallback: the URL controls both
    // claims and resolution, so local mode needs a bank seeded from warm skills.
    let cache_dir = harness
        .artifacts_dir
        .parent()
        .unwrap()
        .join("home/.config/gah/skill-cache");
    let caches: Vec<_> = fs::read_dir(cache_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(caches.len(), 1);
    let warmed: Value = serde_json::from_slice(&fs::read(&caches[0]).unwrap()).unwrap();
    assert_eq!(warmed, resolution());
    harness.use_local_skills(&warmed);
    let local = harness.run_one_loop().unwrap();
    assert_skill_argv(&harness);
    assert!(
        local
            .ledger_entries
            .iter()
            .any(|entry| entry["mode"] == "claim" && entry["work_id"] == "TICKET-1516"),
        "{local:?}"
    );
    assert!(
        local
            .ledger_entries
            .iter()
            .any(|entry| entry["backend"] == "codex" && entry["backend_exit_code"] == 1),
        "{local:?}"
    );
    let status = harness.run_status_json().unwrap();
    assert_eq!(status["node"]["role"], "standalone");
    assert_inventory_status(&status);
}
