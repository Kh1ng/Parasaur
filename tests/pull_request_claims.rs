//! Issue #1430: review and fix work claims its pull request through the real
//! controller path (`gah loop --once`), and a single worker moves past a pull
//! request a person holds to the next item within the same cycle.

mod support;

use sha2::{Digest, Sha256};
use support::fake_ledger::{ledger_entry_full, TestLedger};
use support::scenario::ScenarioHarness;

/// One open pull request of the fake repository.
struct Pull {
    number: u64,
    branch: &'static str,
    labels: &'static [&'static str],
    /// Assigned by hand to a person, so no loop may claim it.
    held_by_person: bool,
    /// A person is assigned once this loop has claimed it. The claim is
    /// posted half a TTL old, so the lease renews, and sees the person, at
    /// once.
    taken_over_while_working: bool,
    /// The branch's head commit, once the branch is pushed.
    sha: String,
}

impl Pull {
    fn title(&self) -> String {
        format!("Draft: TICKET-00{} Change", self.number)
    }
}

/// `sync::review_metadata_fingerprint` of `pull` as the fake reports it.
fn metadata_fingerprint(pull: &Pull) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"gah-review-metadata-v1\0");
    let title = pull.title();
    let title = title.strip_prefix("Draft: ").unwrap();
    for value in [pull.sha.as_str(), title, ""] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    hasher.update([1u8]);
    format!("sha256:{:x}", hasher.finalize())
}

/// A completed NEEDS_FIX review of `pull`'s current source, which makes the
/// controller select a repair of it.
fn needs_fix_review(pull: &Pull) -> serde_json::Value {
    let mut entry = ledger_entry_full(
        "review",
        pull.branch,
        Some("review"),
        &format!("TICKET-00{}", pull.number),
        "2026-07-01T00:00:00Z",
    );
    let fingerprint = metadata_fingerprint(pull);
    entry["review_verdict"] = serde_json::json!("NEEDS_FIX");
    entry["review_source_sha"] = serde_json::json!(pull.sha);
    entry["review_metadata_fingerprint"] = serde_json::json!(fingerprint);
    entry["review_contract_version"] =
        serde_json::json!(git_agent_harness::ledger::REVIEW_CONTRACT_VERSION);
    entry["review_generation"] = serde_json::json!(git_agent_harness::ledger::review_generation(
        Some(&pull.sha),
        Some(&fingerprint),
    ));
    entry["review_blocking_findings"] = serde_json::json!(["stability regression"]);
    entry
}

/// Seed the shared forge with the original claim fixtures. A scripted actor
/// joins on the third assignee read, after the stake and settle reads.
fn install_gh(harness: &mut ScenarioHarness, pulls: &[Pull]) -> std::path::PathBuf {
    let forge = harness.stateful_forge();
    for pull in pulls {
        forge.pull(pull.number, pull.branch, &pull.sha, &pull.title());
        forge.update(|state| {
            let item = &mut state["pulls"][pull.number.to_string()];
            item["labels"] = serde_json::json!(pull.labels);
            if pull.held_by_person {
                item["assignees"] = serde_json::json!(["a-person"]);
            }
            state["checks"][&pull.sha] = serde_json::json!([
                {"status": "completed", "conclusion": "success"}
            ]);
            if pull.taken_over_while_working {
                let posted_at = (time::OffsetDateTime::now_utc()
                    - time::Duration::minutes(30))
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap();
                // Age the recorded comment before settle reads it. This also
                // works when #1515 supplies GAH_TEST_NOW to the fake process.
                state["effects"].as_array_mut().unwrap().push(serde_json::json!({
                    "endpoint": format!("repos/owner/repo/issues/{}/comments", pull.number),
                    "method": "GET", "at": 2, "argv_contains": ["--paginate"],
                    "patches": [
                        {"path": ["pulls", pull.number.to_string(), "comments", 0, "created_at"], "value": posted_at},
                        {"path": ["pulls", pull.number.to_string(), "comments", 0, "updated_at"], "value": posted_at}
                    ]
                }));
                state["effects"]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::json!({
                        "endpoint": format!("repos/owner/repo/issues/{}", pull.number),
                        "method": "GET", "at": 3,
                        "patches": [{"path": ["pulls", pull.number.to_string(), "assignees"],
                                     "value": ["gah-bot", "a-person"]}]
                    }));
            }
        });
    }
    forge.log_path.clone()
}

/// Every worker records that it ran, in the same log as `gh`, then runs
/// `body`.
fn install_workers(harness: &ScenarioHarness, state: &std::path::Path, body: &str) {
    for worker in ["openhands", "vibe", "opencode", "claude", "codex", "agy"] {
        support::write_executable(
            &harness.bin_dir.join(worker),
            &format!(
                "#!/bin/sh\necho 'WORKER {worker}' >> '{}'\n{body}\n",
                state.display()
            ),
        );
    }
}

/// The ledger a repair of `pull` has used up its attempts on: its review and
/// as many repairs of that review as the default cap allows.
fn fix_capped(pull: &Pull) -> Vec<serde_json::Value> {
    let review = needs_fix_review(pull);
    let repairs: Vec<_> = (0..2)
        .map(|attempt| {
            let mut repair = ledger_entry_full(
                "fix",
                pull.branch,
                Some("post_review_repair"),
                &format!("TICKET-00{}", pull.number),
                &format!("2026-07-01T0{}:00:00Z", attempt + 1),
            );
            repair["review_contract_version"] = review["review_contract_version"].clone();
            repair["review_generation"] = review["review_generation"].clone();
            repair
        })
        .collect();
    [vec![review], repairs].concat()
}

/// Run one loop cycle over `pulls` with provider claims on and the ledger
/// entries `ledger_for` returns for each pull request. Returns the `gh` and
/// worker log, one call per line, with the loop's events.
fn run_cycle(
    pulls: &mut [Pull],
    ledger_for: impl Fn(&Pull) -> Vec<serde_json::Value>,
) -> (Vec<String>, Vec<serde_json::Value>) {
    run_cycle_with_workers(pulls, ledger_for, FAILING_WORKER)
}

/// A worker that fails at once.
const FAILING_WORKER: &str = "echo 'worker failed' >&2\nexit 23";

fn run_cycle_with_workers(
    pulls: &mut [Pull],
    ledger_for: impl Fn(&Pull) -> Vec<serde_json::Value>,
    worker: &str,
) -> (Vec<String>, Vec<serde_json::Value>) {
    let harness = ScenarioHarness::new("github").with_config_append(
        "\n[profiles.test.publishing.issue_claim]\nmode = \"github_assignee\"\nverify_seconds = 0\n",
    );
    let mut ledger = TestLedger::new();
    for pull in pulls.iter_mut() {
        harness.create_remote_branch(pull.branch);
        let head = std::process::Command::new("git")
            .arg("-C")
            .arg(&harness.local_repo_dir)
            .args(["rev-parse", pull.branch])
            .output()
            .unwrap();
        pull.sha = String::from_utf8(head.stdout).unwrap().trim().to_string();
        for entry in ledger_for(pull) {
            ledger = ledger.with_entry(entry);
        }
    }
    let mut harness = harness.with_ledger(ledger);
    let state = install_gh(&mut harness, pulls);
    install_workers(&harness, &state, worker);

    let result = harness.run_one_loop().unwrap();

    let log = std::fs::read_to_string(state).unwrap();
    eprintln!("{log}\n{result:?}");
    (log.lines().map(str::to_string).collect(), result.events)
}

/// Asserts the person's pull request `held` was skipped untouched and
/// `claimed` was claimed (assignee, comment, re-read after the verify pause)
/// before any worker ran.
fn assert_moved_past_and_claimed(
    calls: &[String],
    events: &[serde_json::Value],
    held: u64,
    claimed: u64,
) {
    let position = |needle: &str| calls.iter().position(|call| call.contains(needle));
    assert!(
        !calls
            .iter()
            .any(|call| call.contains(&format!("issues/{held}/"))
                && (call.contains("POST") || call.contains("DELETE"))),
        "a person's pull request was written to"
    );
    assert!(events
        .iter()
        .any(|event| event["details"].as_str().is_some_and(
            |details| details.contains(&format!("pull request #{held}: held by a-person"))
        )));
    let assigned = position(&format!(
        "--method POST repos/owner/repo/issues/{claimed}/assignees"
    ))
    .expect("the next pull request was not assigned");
    let commented = position(&format!(
        "--method POST repos/owner/repo/issues/{claimed}/comments"
    ))
    .expect("the next pull request got no claim comment");
    let worked = position("WORKER ").expect("no worker ran");
    assert!(assigned < commented && commented < worked);
    let reread = format!("repos/owner/repo/issues/{claimed} --jq .assignees[].login");
    assert!(calls[commented..worked]
        .iter()
        .any(|call| call.ends_with(&reread)));
    // Later attempts reuse the claim: nothing claims the pull request twice.
    let writes = format!("--method POST repos/owner/repo/issues/{claimed}/");
    assert_eq!(
        calls.iter().filter(|call| call.contains(&writes)).count(),
        2
    );
}

#[test]
fn a_review_moves_past_a_pull_request_a_person_holds_and_claims_the_next() {
    let mut pulls = [
        Pull {
            number: 7,
            branch: "gah/a-held-review",
            labels: &[],
            held_by_person: true,
            taken_over_while_working: false,
            sha: String::new(),
        },
        Pull {
            number: 8,
            branch: "gah/b-free-review",
            labels: &[],
            held_by_person: false,
            taken_over_while_working: false,
            sha: String::new(),
        },
    ];

    let (calls, events) = run_cycle(&mut pulls, |_| Vec::new());

    assert_moved_past_and_claimed(&calls, &events, 7, 8);
}

#[test]
fn a_fix_claims_its_pull_request_after_a_held_review_is_skipped() {
    let mut pulls = [
        Pull {
            number: 7,
            branch: "gah/a-held-review",
            labels: &[],
            held_by_person: true,
            taken_over_while_working: false,
            sha: String::new(),
        },
        Pull {
            number: 9,
            branch: "gah/c-needs-fix",
            labels: &["gah-needs-fix"],
            held_by_person: false,
            taken_over_while_working: false,
            sha: String::new(),
        },
    ];

    let (calls, events) = run_cycle(&mut pulls, |pull| match pull.number {
        9 => vec![needs_fix_review(pull)],
        _ => Vec::new(),
    });

    assert!(
        calls
            .iter()
            .any(|call| call.contains("head=owner:gah/c-needs-fix")),
        "the repair never looked up its pull request"
    );
    assert_moved_past_and_claimed(&calls, &events, 7, 9);
}

#[test]
fn a_pull_request_at_its_repair_cap_is_never_claimed() {
    let mut pulls = [Pull {
        number: 9,
        branch: "gah/c-needs-fix",
        labels: &["gah-needs-fix"],
        held_by_person: false,
        taken_over_while_working: false,
        sha: String::new(),
    }];

    let (calls, _) = run_cycle(&mut pulls, fix_capped);

    // The retry cap gates the repair before selection, so the claim
    // protocol never even reads the pull request's assignees.
    assert!(!calls.iter().any(|call| call.contains("issues/9")));
    assert!(!calls.iter().any(|call| call.starts_with("WORKER ")));
}

#[test]
fn a_review_whose_claim_is_taken_over_while_it_works_publishes_nothing() {
    let mut pulls = [Pull {
        number: 8,
        branch: "gah/b-free-review",
        labels: &[],
        held_by_person: false,
        taken_over_while_working: true,
        sha: String::new(),
    }];

    // The reviewer outlasts the lease's first renewal, which finds the person.
    let (calls, events) = run_cycle_with_workers(
        &mut pulls,
        |_| Vec::new(),
        "sleep 3\necho '{\"verdict\":\"APPROVE\",\"confidence\":\"high\"}'",
    );

    assert!(calls.iter().any(|call| call.starts_with("WORKER ")));
    assert!(events.iter().any(|event| event["details"]
        .as_str()
        .is_some_and(|details| details.contains("pull request #8: lease taken over by a-person"))));
    // Only the claim itself was ever written: no verdict, no label, no
    // renewal of a claim that is no longer this loop's.
    let writes: Vec<&String> = calls
        .iter()
        .filter(|call| {
            ["POST", "PATCH", "PUT", "DELETE"]
                .iter()
                .any(|method| call.contains(&format!("--method {method}")))
        })
        .collect();
    assert_eq!(writes.len(), 2, "{writes:?}");
    assert!(writes[0].contains("repos/owner/repo/issues/8/assignees"));
    assert!(writes[1].contains("repos/owner/repo/issues/8/comments"));
    assert!(!calls.iter().any(|call| call.contains("labels")));
}
