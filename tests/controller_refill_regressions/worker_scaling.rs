//! Scenario 8 (#1529): the loop's concurrency follows the quota-scaled worker
//! limit, a manual boost raises it, a later weaker quota reading lowers it, and
//! node-pressure admission composes with it (the lower one wins).

use super::watchdog::wait_with_output;
use super::{
    make_fake_bin_with_body, read_u32_file, serial, setup_fix_dispatch_repo, spawn_bin,
    test_tempdir, write_node_pressure_with_load, ProcessGroupGuard,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::thread;
use std::time::{Duration, Instant};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

struct Scaled {
    tmp: tempfile::TempDir,
    cfg: PathBuf,
    home: PathBuf,
    repo: PathBuf,
    fake_bin: PathBuf,
    quota: PathBuf,
    active: PathBuf,
    max: PathBuf,
    release: PathBuf,
}

/// Baseline 1 worker; automatic scaling adds up to 2 for `codex/local/test`
/// while its freshest quota window keeps at least 50% left, capped at 3.
fn scaled() -> Scaled {
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(
        &tmp,
        "validation_commands = [\"true\"]\nmax_parallel_workers = 1\n\
         [profiles.real.worker_scaling]\nenabled = true\nextra_per_model = 2\nmax_workers = 3\nmin_remaining_percent = 50.0\n",
    );
    let config = fs::read_to_string(&cfg).unwrap().replace(
        "improve_backend = \"codex\"",
        "improve_backend = \"codex\"\nimprove_candidates = [{ backend = \"codex\", model = \"local/test\" }]",
    );
    fs::write(
        &cfg,
        config.replace(
            "[profiles.real.routing]",
            "[profiles.real.max_concurrent_per_model]\n\"codex/local/test\" = 5\n\n[profiles.real.routing]",
        ),
    )
    .unwrap();
    let fake_bin = tmp.path().join("bin");
    fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        "#!/bin/sh\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"list\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then printf 'https://github.com/owner/real/pull/1\\n'; exit 0; fi\nexit 0\n",
    );
    let active = tmp.path().join("active");
    let max = tmp.path().join("max-active");
    let release = tmp.path().join("release");
    let lock = tmp.path().join("active.lock");
    // Every worker blocks until the test releases it, so the number of
    // concurrently running workers is exactly what the loop admitted.
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\nlock='{lock}'\nacquire() {{ while ! mkdir \"$lock\" 2>/dev/null; do sleep 0.01; done; }}\n\
             acquire\ncount=$( [ -f '{active}' ] && cat '{active}' || echo 0 )\ncount=$((count + 1))\necho \"$count\" > '{active}'\n\
             max=$( [ -f '{max}' ] && cat '{max}' || echo 0 )\nif [ \"$count\" -gt \"$max\" ]; then echo \"$count\" > '{max}'; fi\nrmdir \"$lock\"\n\
             printf 'agent edit\\n' >> \"agent-edit-$$.txt\"\n\
             while [ ! -f '{release}' ]; do sleep 0.05; done\n\
             acquire\ncount=$(cat '{active}')\ncount=$((count - 1))\necho \"$count\" > '{active}'\nrmdir \"$lock\"\nexit 0\n",
            lock = lock.display(),
            active = active.display(),
            max = max.display(),
            release = release.display(),
        ),
    );
    Scaled {
        quota: tmp.path().join("quota-store.jsonl"),
        tmp,
        cfg,
        home,
        repo,
        fake_bin,
        active,
        max,
        release,
    }
}

fn reading(path: &Path, remaining: f64, observed_at: OffsetDateTime) {
    let line = serde_json::json!({
        "backend": "codex", "model": "local/test", "quota_window": "weekly",
        "quota_remaining_percent": remaining,
        "observed_at": observed_at.format(&Rfc3339).unwrap(),
        "checked_at": observed_at.format(&Rfc3339).unwrap()
    });
    let mut text = fs::read_to_string(path).unwrap_or_default();
    text.push_str(&line.to_string());
    text.push('\n');
    fs::write(path, text).unwrap();
}

fn tickets(repo: &Path, ids: std::ops::RangeInclusive<u32>) {
    let dir = repo.join("docs/tickets");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for id in ids {
        fs::write(
            dir.join(format!("TICKET-{id}-scaling.md")),
            format!("# TICKET-{id}: Scaling backlog\n\nGoal: occupy one worker slot.\nRecommended backend: codex\n"),
        )
        .unwrap();
    }
}

/// One `--once` pass without `--parallel`, so the pool size is the profile's
/// scaled limit. Returns the controller's stderr once every worker is released.
fn pass(
    s: &Scaled,
    now_override: Option<OffsetDateTime>,
    expected_workers: u32,
    logical_cpus: Option<u64>,
) -> String {
    for path in [&s.active, &s.max, &s.release] {
        let _ = fs::remove_file(path);
    }
    let mut command = spawn_bin(s.tmp.path());
    if let Some(cpus) = logical_cpus {
        // After spawn_bin wrote its generous default fixture.
        write_node_pressure_with_load(&s.tmp.path().join("node-pressure.json"), 60, cpus, 0.5);
    }
    command
        .args([
            "loop",
            "--profile",
            "real",
            "--config-path",
            s.cfg.to_str().unwrap(),
            "--once",
        ])
        .env(
            "PATH",
            format!(
                "{}:{}",
                s.fake_bin.display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .env("HOME", &s.home)
        .env("GITHUB_TOKEN", "token")
        .env("GAH_QUOTA_STORE_PATH", &s.quota)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(now) = now_override {
        command.env("GAH_TEST_NOW", now.format(&Rfc3339).unwrap());
    }
    let child = ProcessGroupGuard::new(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(30);
    while read_u32_file(&s.max) < expected_workers {
        assert!(
            Instant::now() < deadline,
            "only {} workers started, expected {expected_workers}",
            read_u32_file(&s.max)
        );
        thread::sleep(Duration::from_millis(20));
    }
    // Give the loop time to over-admit if it were going to.
    thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        read_u32_file(&s.max),
        expected_workers,
        "concurrency exceeded the limit"
    );
    fs::write(&s.release, "release").unwrap();
    let output = wait_with_output(child);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The limits `gah status` derives from the same profile and quota store.
fn limits(s: &Scaled, now_override: Option<OffsetDateTime>) -> serde_json::Value {
    let mut command = spawn_bin(s.tmp.path());
    command
        .args([
            "status",
            "--profile",
            "real",
            "--config-path",
            s.cfg.to_str().unwrap(),
            "--json",
        ])
        .env(
            "PATH",
            format!(
                "{}:{}",
                s.fake_bin.display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .env("HOME", &s.home)
        .env("GITHUB_TOKEN", "token")
        .env("GAH_QUOTA_STORE_PATH", &s.quota);
    if let Some(now) = now_override {
        command.env("GAH_TEST_NOW", now.format(&Rfc3339).unwrap());
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    status["worker_limits"].clone()
}

fn notes(limits: &serde_json::Value) -> String {
    limits["notes"]
        .as_array()
        .map(|notes| {
            notes
                .iter()
                .filter_map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
}

#[test]
fn parallel_loop_follows_quota_scaled_limit_then_boost_then_a_weaker_reading() {
    let _lock = serial();
    let s = scaled();
    let now = OffsetDateTime::now_utc();

    // 80% left in the freshest window: baseline 1 + 2 scaled = 3 of 4 backlog items.
    reading(&s.quota, 80.0, now);
    tickets(&s.repo, 601..=604);
    let report = limits(&s, None);
    assert_eq!(report["workers"], 3, "{report}");
    assert!(notes(&report).contains("codex/local/test: +2"), "{report}");
    pass(&s, None, 3, None);

    // A manual boost adds on top of the scaled limit.
    let boost = spawn_bin(s.tmp.path())
        .args([
            "profile",
            "set",
            "real",
            "--boost-workers",
            "1",
            "--boost-hours",
            "1",
            "--config",
            s.cfg.to_str().unwrap(),
        ])
        .env("HOME", &s.home)
        .output()
        .unwrap();
    assert!(boost.status.success(), "{boost:?}");
    tickets(&s.repo, 611..=614);
    let report = limits(&s, None);
    assert_eq!(report["workers"], 4, "{report}");
    assert!(notes(&report).contains("manual boost: +1"), "{report}");
    pass(&s, None, 4, None);

    // Two hours later the boost has expired and the freshest reading is 10%:
    // automatic scaling withdraws and the loop admits only the baseline.
    let later = now + time::Duration::hours(2);
    reading(&s.quota, 10.0, later - time::Duration::minutes(1));
    tickets(&s.repo, 621..=624);
    let report = limits(&s, Some(later));
    assert_eq!(report["workers"], 1, "{report}");
    assert!(
        notes(&report).contains("not scaled (10% left, needs 50%)"),
        "{report}"
    );
    assert!(
        notes(&report).contains("manual boost has expired"),
        "{report}"
    );
    pass(&s, Some(later), 1, None);
}

#[test]
fn node_pressure_admission_composes_with_the_scaled_limit_and_the_lower_wins() {
    let _lock = serial();
    let s = scaled();
    reading(&s.quota, 80.0, OffsetDateTime::now_utc());
    tickets(&s.repo, 631..=634);
    // Live free memory never drops for fake workers, so memory admission
    // cannot see them; CPU admission does: on two logical CPUs the first
    // worker is always allowed and the second crosses the CPU reserve.
    let report = limits(&s, None);
    assert_eq!(
        report["workers"], 3,
        "scaling alone would allow 3: {report}"
    );
    let stderr = pass(&s, None, 1, Some(2));
    assert!(
        stderr.contains("node CPU reserve would be crossed"),
        "{stderr}"
    );
}
