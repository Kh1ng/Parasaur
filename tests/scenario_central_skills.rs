//! Scenario 9 (#1532): a skill installed and bound on central reaches a
//! worker dispatch, a binding change shows on the next dispatch with no
//! restart, and a central outage dispatches from the warm cache whose age
//! `gah status` reports.

mod support;

use serde_json::{json, Value};
use std::{fs, process::Command};
use support::scenario::{central::FakeCentral, CommandResult, ScenarioHarness};

const TICKET: &str = "docs/tickets/TICKET-1532-central-skill.md";

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
    // A model rejection ends each dispatch after one attempt and leaves its argv behind.
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
        "# TICKET-1532: Exercise central skills\n\nGoal: verify a central skill reaches a worker.\n\nRecommended backend: codex\n").unwrap();
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
        state["quotas"]["test"] = json!({"profile":"test", "since":"7d", "nodes":[]});
    });
    harness
}

fn install(central: &FakeCentral, id: &str, version: &str, content: &str) {
    let (status, _) = central.request(
        "POST",
        "/api/skills",
        &json!({"id": id, "version": version, "content": content, "backends": ["codex"]}),
    );
    assert_eq!(status, 201);
}

fn bind(central: &FakeCentral, ids: &[&str]) {
    let (status, _) = central.request(
        "PUT",
        "/api/skills/bindings",
        &json!({"profile":"test", "backend":"codex", "instance":"worker-codex", "bindings": ids}),
    );
    assert_eq!(status, 200);
}

fn dispatch(harness: &mut ScenarioHarness, target: &str) -> CommandResult {
    let marker = harness.artifacts_dir.join("worker-argv");
    let _ = fs::remove_file(&marker);
    let result = harness
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
            target,
        ])
        .unwrap();
    assert!(
        marker.exists(),
        "dispatch did not reach the worker: {result:?}"
    );
    result
}

fn argv(harness: &ScenarioHarness) -> String {
    fs::read_to_string(harness.artifacts_dir.join("worker-argv")).unwrap()
}

fn resolve_calls(central: &FakeCentral) -> usize {
    central.state()["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| {
            call["method"] == "GET"
                && call["target"]
                    .as_str()
                    .unwrap()
                    .starts_with("/api/skills/resolve?")
        })
        .count()
}

#[test]
fn skill_installed_and_bound_on_central_reaches_the_worker_dispatch() {
    let central = FakeCentral::new();
    let mut harness = worker(&central);
    install(
        &central,
        "central-skill",
        "1.0.0",
        "Follow the central skill.",
    );
    bind(&central, &["central-skill"]);

    let loop_result = harness.run_one_loop().unwrap();
    assert!(
        harness.artifacts_dir.join("worker-argv").exists(),
        "{loop_result:?}"
    );
    assert!(
        argv(&harness).contains("## central-skill@1.0.0\nFollow the central skill."),
        "{}",
        argv(&harness)
    );
    assert_eq!(resolve_calls(&central), 1);
    let installed = central.state()["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| call["method"] == "POST" && call["target"] == "/api/skills")
        .count();
    assert_eq!(installed, 1);
}

#[test]
fn changing_the_binding_shows_on_the_next_dispatch_without_restart() {
    let central = FakeCentral::new();
    let mut harness = worker(&central);
    install(
        &central,
        "central-skill",
        "1.0.0",
        "Follow the central skill.",
    );
    install(&central, "other-skill", "2.0.0", "Follow the other skill.");
    bind(&central, &["central-skill"]);
    dispatch(&mut harness, "first dispatch");
    assert!(
        argv(&harness).contains("## central-skill@1.0.0"),
        "{}",
        argv(&harness)
    );
    assert!(
        !argv(&harness).contains("other-skill"),
        "{}",
        argv(&harness)
    );

    bind(&central, &["other-skill"]);
    dispatch(&mut harness, "second dispatch");
    assert!(
        argv(&harness).contains("## other-skill@2.0.0\nFollow the other skill."),
        "{}",
        argv(&harness)
    );
    assert!(
        !argv(&harness).contains("central-skill"),
        "{}",
        argv(&harness)
    );

    // A newer install of the bound id wins on the next dispatch too.
    install(
        &central,
        "other-skill",
        "2.1.0",
        "Follow the newer other skill.",
    );
    bind(&central, &["other-skill"]);
    dispatch(&mut harness, "third dispatch");
    assert!(
        argv(&harness).contains("## other-skill@2.1.0"),
        "{}",
        argv(&harness)
    );
    assert_eq!(
        resolve_calls(&central),
        3,
        "one resolve per dispatch, no restart"
    );
}

#[test]
fn central_outage_dispatches_from_the_warm_cache_and_status_reports_its_age() {
    let mut central = FakeCentral::new();
    let mut harness = worker(&central);
    install(
        &central,
        "central-skill",
        "1.0.0",
        "Follow the central skill.",
    );
    bind(&central, &["central-skill"]);
    dispatch(&mut harness, "warm the cache");
    assert!(
        argv(&harness).contains("## central-skill@1.0.0"),
        "{}",
        argv(&harness)
    );
    central.shutdown();

    let cached = dispatch(&mut harness, "dispatch during outage");
    assert!(
        cached.stderr.contains("using last-known-good cache"),
        "{cached:?}"
    );
    assert!(
        argv(&harness).contains("## central-skill@1.0.0"),
        "{}",
        argv(&harness)
    );

    let status = harness.run_status_json().unwrap();
    let inventory = status["skill_inventory"]
        .as_array()
        .unwrap()
        .iter()
        .find(|view| view["backend_instance"] == "worker-codex")
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        inventory["bound_skill_ids"],
        json!(["central-skill"]),
        "{status}"
    );
    assert!(
        inventory["skill_cache_resolved_at"].is_string(),
        "{inventory}"
    );
    let age = inventory["skill_cache_age_seconds"].as_i64().unwrap();
    assert!((0..600).contains(&age), "{inventory}");
}
