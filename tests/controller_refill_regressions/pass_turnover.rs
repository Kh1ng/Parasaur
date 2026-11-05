use super::*;

#[test]
fn busy_queue_releases_pass_state_and_remaining_work_runs_in_the_next_pass() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 901..907 {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{id}-turnover.md")),
            format!("# TICKET-{id}: Pass turnover\n\nGoal: verify bounded scheduling passes.\nRecommended backend: codex\n"),
        ).unwrap();
    }
    let fake_bin = tmp.path().join("bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let calls = tmp.path().join("calls");
    make_fake_bin_with_body(&fake_bin, "gh", "#!/bin/sh\nif [ \"$1\" = pr ] && [ \"$2\" = list ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = api ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = pr ] && [ \"$2\" = create ]; then echo 'https://github.com/owner/real/pull/1'; fi\n");
    make_fake_bin_with_body(&fake_bin, "codex", &format!(
        "#!/bin/sh\nprintf 'call\\n' >> '{}'\nprintf 'fixture edit\\n' > pass-turnover.txt\nprintf 'done\\n'\n",
        calls.display(),
    ));
    let ledger = tmp.path().join("ledger.jsonl");
    for expected_calls in [4, 6] {
        let child = ProcessGroupGuard::new(
            spawn_bin(tmp.path())
                .args([
                    "loop",
                    "--profile",
                    "real",
                    "--config-path",
                    cfg.to_str().unwrap(),
                    "--once",
                    "--parallel",
                    "2",
                ])
                // Production admits 128 actions (or one larger wave). The
                // debug-only limit exercises turnover with six fixture jobs.
                .env("GAH_TEST_PARALLEL_PASS_ACTION_LIMIT", "4")
                .env(
                    "PATH",
                    format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
                )
                .env("HOME", &home)
                .env("GITHUB_TOKEN", "token")
                .env("GAH_LEDGER_PATH", &ledger)
                .env("GAH_EVENTS_PATH", tmp.path().join("events.jsonl"))
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let output = watchdog::wait_with_output(child);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(&calls).unwrap().lines().count(),
            expected_calls
        );
        if expected_calls == 4 {
            assert!(String::from_utf8_lossy(&output.stderr)
                .contains("4-action limit; active workers drained"));
        }
    }
}
