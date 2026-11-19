use super::*;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::time::{Duration, Instant};
use support::ProcessGroupGuard;

#[test]
fn loop_sigterm_resumes_the_committed_checkpoint_and_current_issue_instructions() {
    let mut harness = harness();
    let ready = harness.artifacts_dir.join("worker-committed");
    let prompts = harness.artifacts_dir.join("prompts");
    fs::create_dir(&prompts).unwrap();
    support::write_executable(
        &harness.bin_dir.join("agy"),
        &format!(
            r#"#!/bin/sh
set -eu
case "$1" in --version) echo 'test 1.0'; exit 0;; models|--help) exit 0;; esac
if [ "$2" = /usage ]; then exit 0; fi
if [ ! -f '{ready}' ]; then
  printf '%s\n' "$@" > '{prompts}/first'
  echo 'partial implementation' > partial.txt
  git add partial.txt
  git commit -qm 'Partial implementation'
  git rev-parse HEAD > '{ready}.tmp'
  mv '{ready}.tmp' '{ready}'
  exec sleep 60
fi
git rev-parse HEAD > '{prompts}/resumed-head'
printf '%s\n' "$@" > '{prompts}/second'
[ "$(cat partial.txt)" = 'partial implementation' ]
echo completed > completed.txt
echo 'Implemented the remaining changes'
"#,
            ready = ready.display(),
            prompts = prompts.display()
        ),
    );

    // Reuse the prepared command so the restart reads the real first process's
    // ledger, claim store, checkpoints and forge state.
    let mut cmd = harness.prepare_loop_command(false);
    cmd.process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ProcessGroupGuard::new(cmd.spawn().unwrap());
    let start = Instant::now();
    while !ready.exists() {
        if child.try_wait().unwrap().is_some() {
            let output = child.wait_with_output().unwrap();
            panic!(
                "loop exited before commit: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "worker did not commit"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    const STEER: &str = "Preserve the existing retry counter in the resumed implementation.";
    let updated = harness
        .stateful_forge()
        .command("gh")
        .args([
            "api",
            "repos/owner/repo/issues/1",
            "--method",
            "PATCH",
            "-f",
            &format!("body={STEER}"),
        ])
        .output()
        .unwrap();
    assert!(updated.status.success(), "{updated:?}");
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGTERM) }, 0);
    let stop = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(
            stop.elapsed() < Duration::from_secs(30),
            "SIGTERM failed to drain the loop"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let first = child.wait_with_output().unwrap();
    let entries = TestLedger::read_from(&harness.ledger_path).unwrap();
    assert_eq!(
        entries.len(),
        1,
        "first: {}\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let entry = &entries[0];
    let checkpoint = entry["resumable_checkpoint_branch"]
        .as_str()
        .unwrap_or_else(|| panic!("{entry}"));
    let partial_sha = fs::read_to_string(&ready).unwrap();
    assert_eq!(
        git(&harness, &["rev-parse", checkpoint]),
        partial_sha.trim()
    );
    assert!(harness.stateful_forge().state()["pulls"]
        .as_object()
        .unwrap()
        .is_empty());

    let second = cmd.output().unwrap();
    assert!(
        second.status.success(),
        "resumed: {}\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        prompts.join("second").exists(),
        "missing resumed worker: {}\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        fs::read_to_string(prompts.join("resumed-head")).unwrap(),
        partial_sha
    );
    assert!(!fs::read_to_string(prompts.join("first"))
        .unwrap()
        .contains(STEER));
    assert!(fs::read_to_string(prompts.join("second"))
        .unwrap()
        .contains(STEER));
    let entries = TestLedger::read_from(&harness.ledger_path).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1]["mr_created"], true);
    let state = harness.stateful_forge().state();
    assert_eq!(state["pulls"].as_object().unwrap().len(), 1);
    assert_eq!(state["issues"]["1"]["assignees"], json!(["gah-bot"]));
    let claims: Vec<_> = state["issues"]["1"]["comments"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|comment| {
            comment["body"]
                .as_str()
                .is_some_and(|body| body.starts_with("<!-- gah-claim "))
        })
        .collect();
    assert_eq!(claims.len(), 1, "the restart duplicated the forge claim");
    let created = harness
        .stateful_forge()
        .calls()
        .into_iter()
        .filter(|args| args.starts_with(&["gh".into(), "pr".into(), "create".into()]))
        .count();
    assert_eq!(created, 1);
}
