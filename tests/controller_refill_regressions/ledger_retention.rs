use super::*;
use std::io::Write;

#[test]
#[ignore = "controlled memory probe; requires GAH_TEST_LEDGER_MEMORY_RESULT"]
fn active_workers_do_not_need_full_ledger_snapshots() {
    let _lock = serial();
    let result =
        std::env::var("GAH_TEST_LEDGER_MEMORY_RESULT").expect("measurement output path required");
    let workers: usize = std::env::var("GAH_TEST_LEDGER_MEMORY_WORKERS")
        .unwrap_or_else(|_| "4".into())
        .parse()
        .unwrap();
    assert!([1, 4].contains(&workers));
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 911..915 {
        fs::write(repo.join(format!("docs/tickets/TICKET-{id}-memory.md")), format!("# TICKET-{id}: Ledger ownership\n\nGoal: measure worker ledger retention.\nRecommended backend: codex\n")).unwrap();
    }
    let config: git_agent_harness::config::GahConfig =
        toml::from_str(&fs::read_to_string(&cfg).unwrap()).unwrap();
    let mut entry = git_agent_harness::ledger::LedgerEntry::new(
        "unrelated",
        &config.profiles["real"],
        "codex",
        "fix",
        "fixture",
        None,
        None,
    );
    entry.error_summary = Some("x".repeat(512 * 1024));
    let ledger = tmp.path().join("ledger.jsonl");
    let mut file = fs::File::create(&ledger).unwrap();
    for id in 0..64 {
        entry.work_id = Some(format!("FIXTURE-{id}"));
        serde_json::to_writer(&mut file, &entry).unwrap();
        writeln!(file).unwrap();
    }
    drop(file);
    drop(entry);
    let calls = tmp.path().join("calls");
    fs::create_dir(&calls).unwrap();
    let release = tmp.path().join("release");
    let fake_bin = tmp.path().join("bin");
    fs::create_dir(&fake_bin).unwrap();
    make_fake_bin_with_body(&fake_bin, "gh", "#!/bin/sh\nif [ \"$1\" = pr ] && [ \"$2\" = list ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = api ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = pr ] && [ \"$2\" = create ]; then echo 'https://github.com/owner/real/pull/1'; fi\n");
    make_fake_bin_with_body(&fake_bin, "codex", &format!("#!/bin/sh\nprintf started > '{}/'$$\nwhile [ ! -f '{}' ]; do sleep 0.05; done\nprintf 'fixture edit\\n' > ledger-memory.txt\nprintf 'done\\n'\n", calls.display(), release.display()));
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
                &workers.to_string(),
            ])
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
    let child = wait_for_worker_event(
        child,
        &release,
        || fs::read_dir(&calls).unwrap().count() == workers,
        "memory-fixture workers",
    );
    let status = fs::read_to_string(format!("/proc/{}/status", child.id())).unwrap();
    let kib = |key: &str| -> u64 {
        status
            .lines()
            .find(|line| line.starts_with(key))
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap()
    };
    let measurement = serde_json::json!({"activeWorkers":workers,"ledgerBytes":fs::metadata(&ledger).unwrap().len(),"rssKiB":kib("VmRSS:"),"peakRssKiB":kib("VmHWM:")});
    fs::write(&release, "release").unwrap();
    let output = watchdog::wait_with_output(child);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(result, serde_json::to_vec_pretty(&measurement).unwrap()).unwrap();
}
