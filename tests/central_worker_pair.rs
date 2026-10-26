//! Real worker CLI dispatches against an inspectable, in-process central API.

mod support;

use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, process::Command};
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
    // A model rejection ends the loop after one attempt, without publishing a PR.
    let marker = harness.artifacts_dir.join("worker-argv");
    support::write_executable(&harness.bin_dir.join("codex"), &format!(
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'test 1.0'; exit 0; fi\nprintf '%s\\n' \"$@\" > '{}'\necho 'invalid model selection (--model test-model)'\nexit 1\n", marker.display()
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

// Comparing the whole map rejects duplicate calls and every unlisted route.
fn assert_request_counts(central: &FakeCentral, expected: &[(&str, &str, usize)]) {
    let state = central.state();
    let mut actual = BTreeMap::new();
    for call in state["calls"].as_array().unwrap() {
        let method = call["method"].as_str().unwrap();
        let target = call["target"].as_str().unwrap();
        let path = target.split('?').next().unwrap();
        *actual.entry((method, path)).or_insert(0) += 1;
    }
    let expected: BTreeMap<_, _> = expected
        .iter()
        .filter(|(_, _, count)| *count > 0)
        .map(|&(method, path, count)| ((method, path), count))
        .collect();
    assert_eq!(actual, expected, "request log: {}", state["calls"]);
}

fn assert_worker_dispatch_counts(central: &FakeCentral, quota_requests: usize) {
    assert_request_counts(
        central,
        &[
            ("POST", "/api/registry/nodes", 1),
            ("GET", "/api/registry/fleet", 1),
            ("POST", "/api/claims/acquire", 1),
            ("GET", "/api/skills/resolve", 1),
            ("POST", "/api/claims/release", 1),
            ("POST", "/api/activity", 1),
            ("POST", "/api/claims/renew", 0),
            ("GET", "/api/registry/quota", quota_requests),
        ],
    );
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
    assert_worker_dispatch_counts(&central, 0);
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
    assert_worker_dispatch_counts(&central, 1);
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
    assert_worker_dispatch_counts(&central, 0);
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
    assert_worker_dispatch_counts(&central, 0);
}

#[test]
fn failed_dispatch_releases_central_claim_exactly_once() {
    let central = FakeCentral::new();
    let mut harness = worker(&central);
    let target = harness.local_repo_dir.join(TICKET);
    let result = harness
        .run_dispatch(&[
            "--mode",
            "fix",
            "--backend",
            "codex",
            "--model",
            "test-model",
            "--skip-validation-gate",
            "--retries",
            "0",
            "--target",
            target.to_str().unwrap(),
        ])
        .unwrap();
    assert_ne!(result.exit_code, Some(0), "{result:?}");
    assert_skill_argv(&harness);
    let entries = support::fake_ledger::TestLedger::read_from(&harness.ledger_path).unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| entry["backend"] == "codex" && entry["backend_exit_code"] == 1),
        "{entries:?}"
    );
    assert_request_counts(
        &central,
        &[
            ("POST", "/api/claims/acquire", 1),
            ("GET", "/api/skills/resolve", 1),
            ("POST", "/api/claims/release", 1),
            ("POST", "/api/activity", 1),
            ("POST", "/api/claims/renew", 0),
        ],
    );
    let state = central.state();
    assert_eq!(state["claims"], json!({}));
    let calls = state["calls"].as_array().unwrap();
    assert_eq!(calls[0]["body"]["work_id"], "#1516");
    assert_eq!(calls[3]["body"]["work_id"], "#1516");
    assert_eq!(calls[3]["body"]["node_id"], calls[0]["body"]["node_id"]);
    assert_eq!(calls[3]["body"]["profile"], "test");
}

#[test]
fn fleet_observes_peer_claim_and_worker_does_not_double_claim() {
    let central = FakeCentral::new();
    let mut harness = worker(&central);
    let claimed_at = chrono::Utc::now().to_rfc3339();
    let lease = json!({
        "node_id":"peer-worker", "profile":"test", "work_id":"#1516",
        "claimed_at":claimed_at, "renewed_at":claimed_at, "expires_at":"2099-01-01T00:00:00Z"
    });
    central.update(|state| {
        state["claims"]["test/#1516"] = lease.clone();
        // GET /api/registry/fleet returns NodeObservationSnapshot[], as built
        // by apps/server/src/registryService.ts, not the /fleet/snapshot envelope.
        let claim = json!({"work_id":"#1516", "pid":1516, "scope":"test", "hostname":"peer-host", "claimed_at":claimed_at, "age_seconds":0});
        let mut work = claim.clone();
        work.as_object_mut().unwrap().remove("pid");
        work["node_id"] = json!("peer-worker");
        work["node_qualified_work_id"] = json!("peer-worker:#1516");
        state["fleet"] = json!([{
            "node_id":"peer-worker", "display_name":"Peer worker",
            "advertised_url":"http://127.0.0.1:3774", "version":"0.1.2", "schema_digest":"scenario",
            "state":"healthy", "observed_at":claimed_at, "last_seen_at":claimed_at,
            "last_observed_state":"healthy", "last_error_kind":null, "last_error_message":null,
            "profile":"test", "profiles":["test"], "backend_configured":{"codex":true},
            "backend_instances":[], "availability":[], "recent_ledger":null, "running_workers":[],
            "active_claims":[claim], "active_work":[work], "event_cursor":null,
            "resource_pressure":{"cpu_percent":null, "rss_bytes":null, "disk_percent":null}, "error":null
        }]);
    });
    let result = harness.run_one_loop().unwrap();
    assert!(
        result
            .stderr
            .contains("Peer worker (peer-worker): 1 claim(s), 1 work item(s)"),
        "{result:?}"
    );
    assert!(
        result.stderr.contains("registry_preflight_mode=warn"),
        "{result:?}"
    );
    assert!(
        !harness.artifacts_dir.join("worker-argv").exists(),
        "{result:?}"
    );
    assert!(result.ledger_entries.is_empty(), "{result:?}");
    assert!(
        result.events.iter().any(|event| {
            event["event_type"] == "dispatch_finished"
                && event["details"].as_str().is_some_and(|details| {
                    details.contains("work_id '#1516'")
                        && details.contains("held by node 'peer-worker'")
                })
        }),
        "{result:?}"
    );
    assert_request_counts(
        &central,
        &[
            ("POST", "/api/registry/nodes", 1),
            ("GET", "/api/registry/fleet", 1),
            ("POST", "/api/claims/acquire", 1),
            ("GET", "/api/skills/resolve", 0),
            ("POST", "/api/claims/release", 0),
            ("POST", "/api/claims/renew", 0),
        ],
    );
    let state = central.state();
    assert_eq!(state["claims"], json!({"test/#1516":lease}));
    let calls = state["calls"].as_array().unwrap();
    assert!(calls
        .iter()
        .any(|call| call["target"] == "/api/registry/fleet?profile=test"));
    let acquire = calls
        .iter()
        .find(|call| call["target"] == "/api/claims/acquire")
        .unwrap();
    assert_eq!(acquire["body"]["work_id"], "#1516");
    assert_eq!(acquire["body"]["profile"], "test");
    assert_eq!(acquire["body"]["node_id"], "scenario-worker");
}
