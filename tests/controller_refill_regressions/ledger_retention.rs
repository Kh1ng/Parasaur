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
    let passes: usize = std::env::var("GAH_TEST_LEDGER_MEMORY_PASSES")
        .unwrap_or_else(|_| "1".into())
        .parse()
        .unwrap();
    assert!((1..=48).contains(&passes));
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 911..911 + (workers * passes).max(4) {
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
    let released_workers = tmp.path().join("released-workers");
    fs::create_dir(&released_workers).unwrap();
    let fake_bin = tmp.path().join("bin");
    fs::create_dir(&fake_bin).unwrap();
    make_fake_bin_with_body(&fake_bin, "gh", "#!/bin/sh\nif [ \"$1\" = pr ] && [ \"$2\" = list ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = api ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = pr ] && [ \"$2\" = create ]; then echo 'https://github.com/owner/real/pull/1'; fi\n");
    make_fake_bin_with_body(&fake_bin, "codex", &format!("#!/bin/sh\nprintf started > '{}/'$$\nwhile [ ! -f '{}' ] && [ ! -f '{}/'$$ ]; do sleep 0.05; done\nprintf 'fixture edit\\n' > ledger-memory.txt\nprintf 'done\\n'\n", calls.display(), release.display(), released_workers.display()));
    let mut command = spawn_bin(tmp.path());
    command.args([
        "loop",
        "--profile",
        "real",
        "--config-path",
        cfg.to_str().unwrap(),
        "--parallel",
        &workers.to_string(),
    ]);
    if passes == 1 {
        command.arg("--once");
    } else {
        // One wave per pass lets each sample observe the same live daemon
        // after the preceding pass released its history and worker state.
        command.env("GAH_TEST_PARALLEL_PASS_ACTION_LIMIT", workers.to_string());
        let mut value: toml::Value = toml::from_str(&fs::read_to_string(&cfg).unwrap()).unwrap();
        value["profiles"]["real"].as_table_mut().unwrap().insert(
            "pacing".into(),
            toml::Value::Table(toml::toml! { loop_interval_seconds = 5 }),
        );
        fs::write(&cfg, toml::to_string(&value).unwrap()).unwrap();
    }
    command
        .env(
            "PATH",
            format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("HOME", &home)
        .env("GITHUB_TOKEN", "token")
        .env("GAH_LEDGER_PATH", &ledger)
        .env("GAH_EVENTS_PATH", tmp.path().join("events.jsonl"));
    let stderr_path = std::path::Path::new(&result).with_extension("stderr.log");
    if passes == 1 {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        // Repeated dispatch can fill a pipe before the next sample; logs go
        // directly to retained files instead of accumulating in the parent.
        command
            .stdout(
                fs::File::create(std::path::Path::new(&result).with_extension("stdout.log"))
                    .unwrap(),
            )
            .stderr(fs::File::create(&stderr_path).unwrap());
    }
    let mut child = ProcessGroupGuard::new(command.spawn().unwrap());
    let pid = child.id();
    let started = Instant::now();
    let mut samples = Vec::new();
    for pass in 0..passes {
        child = wait_for_worker_event(
            child,
            &release,
            || fs::read_dir(&calls).unwrap().count() == workers * (pass + 1),
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
        let measurement = serde_json::json!({"activeWorkers":workers,"pass":pass+1,"pid":pid,"elapsedMillis":started.elapsed().as_millis(),"ledgerBytes":fs::metadata(&ledger).unwrap().len(),"rssKiB":kib("VmRSS:"),"peakRssKiB":kib("VmHWM:")});
        samples.push(measurement.clone());
        let report = if passes == 1 {
            measurement
        } else {
            serde_json::json!({"passesRequested":passes,"debugPassActionLimit":workers,"samples":samples})
        };
        fs::write(&result, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        if passes > 1 {
            if pass + 1 == passes {
                let mut value: toml::Value =
                    toml::from_str(&fs::read_to_string(&cfg).unwrap()).unwrap();
                value["defaults"]
                    .as_table_mut()
                    .unwrap()
                    .insert("factory_enabled".into(), false.into());
                fs::write(&cfg, toml::to_string(&value).unwrap()).unwrap();
            }
            for call in fs::read_dir(&calls).unwrap() {
                fs::write(released_workers.join(call.unwrap().file_name()), "release").unwrap();
            }
        }
    }
    fs::write(&release, "release").unwrap();
    let output = watchdog::wait_with_output(child);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    if passes > 1 {
        let log = fs::read_to_string(stderr_path).unwrap();
        assert!(log.matches("action limit; active workers drained").count() >= passes - 1,
            "probe requires real pass boundaries; use the debug binary with its explicit fixture hook");
        assert!(log.contains("factory module disabled; stopping before another iteration"));
    }
}
