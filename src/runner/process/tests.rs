use super::*;
#[cfg(target_os = "linux")]
use crate::runner::backends::test_util::initialize_git_worktree;
use crate::runner::backends::test_util::{fixture, make_fake_bin};
use crate::test_support::ExecGuard;
use std::fs;

#[test]
fn idle_watch_terminates_process_group_on_shutdown_request() {
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    make_fake_bin(
        &f.bin_dir,
        "backend",
        "#!/bin/sh\necho 'started'\nsleep 30\necho 'should not complete'\n",
    );
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));
    let trigger = std::sync::Arc::clone(&shutdown);
    let log_for_trigger = f.session_dir.join("backend-output.log");
    // Fire shutdown only once the backend's first output is confirmed in
    // the log: under heavy parallel-test load the shell can take longer
    // than any fixed delay to start, and a shutdown fired before its
    // first echo would race the log assertion below.
    let trigger_thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if fs::read_to_string(&log_for_trigger)
                .map(|log| log.contains("started"))
                .unwrap_or(false)
            {
                break;
            }
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        trigger.store(true, Ordering::SeqCst);
    });
    let log_path = f.session_dir.join("backend-output.log");

    let result = spawn_with_idle_watch_with_shutdown(
        Command::new(f.bin_dir.join("backend")),
        &log_path,
        &f.worktree,
        60,
        "launching test backend",
        &shutdown,
        true,
    )
    .unwrap();

    trigger_thread.join().unwrap();
    assert_eq!(result.0, -2);
    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains("started"));
    assert!(!log.contains("should not complete"));
    assert!(log.contains("shutdown requested; terminated backend process group"));
}

#[test]
fn hard_timeout_terminates_active_process_group() {
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    let start = time::OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let clock = crate::clock::set_for_test(start);
    make_fake_bin(
        &f.bin_dir,
        "backend-hard-timeout",
        "#!/bin/sh\nif [ -n \"$PARASAUR_INTERNAL_HARD_TIMEOUT_SECONDS\" ]; then echo leaked; exit 99; fi\nwhile :; do echo active; sleep 0.1; done\n",
    );
    let log_path = f.session_dir.join("backend-hard-timeout.log");
    let mut command = Command::new(f.bin_dir.join("backend-hard-timeout"));
    command.env(HARD_TIMEOUT_ENV, "120");
    let handle = clock.handle();
    let advance = thread::spawn(move || {
        let _scope = handle.install();
        thread::sleep(Duration::from_millis(100));
        handle.advance(time::Duration::seconds(120));
    });
    let started = Instant::now();
    let result = spawn_with_idle_watch(
        command,
        &log_path,
        &f.worktree,
        60,
        "launching hard-timeout test backend",
    )
    .unwrap();
    advance.join().unwrap();

    assert_eq!(result.0, -1);
    assert!(started.elapsed() < Duration::from_secs(5));
    let log = fs::read_to_string(log_path).unwrap();
    assert!(!log.contains("leaked"));
    assert!(log.contains("configured 120s hard wall-clock timeout"));
}

#[cfg(unix)]
#[test]
fn normal_exit_reaps_a_background_child_holding_the_log_pipes() {
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    let pid_path = f.session_dir.join("sleeper.pid");
    let body = format!(
        "#!/bin/sh\nsleep 30 &\necho $! > '{}'\necho done\n",
        pid_path.display()
    );
    make_fake_bin(&f.bin_dir, "backend", &body);
    let log_path = f.session_dir.join("backend-output.log");
    let started = Instant::now();
    let command = Command::new(f.bin_dir.join("backend"));
    let result = spawn_with_idle_watch(command, &log_path, &f.worktree, 60, "test").unwrap();

    assert_eq!(result.0, 0);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(fs::read_to_string(log_path).unwrap().contains("done"));
    let pid = crate::runner::pipes::read_pid(&pid_path);
    assert!(crate::runner::pipes::process_exits_within(
        pid,
        Duration::from_secs(2)
    ));
}

#[cfg(target_os = "linux")]
#[test]
fn cleanup_tracks_group_members_after_the_leader_exits() {
    let _exec_guard = ExecGuard::new();
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("child.pid");
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "sleep 30 & echo $! > \"$1\"; exit 0", "fixture"]);
    command.arg(&marker);
    prepare_process_group(&mut command);
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !crate::runner::exited_without_reaping(&child).unwrap() {
        assert!(Instant::now() < deadline, "fixture leader did not exit");
        thread::sleep(Duration::from_millis(5));
    }
    let pid = crate::runner::pipes::read_pid(&marker);
    let snapshot = linux_process_snapshot(pid).unwrap();
    let tracked = linux_descendants(child.id()).contains(&snapshot.identity);
    let cleanup_error = kill_process_group(&mut child);
    let status = child.wait().unwrap();
    assert!(status.success());
    assert_ne!(snapshot.parent_pid, child.id(), "child must be reparented");
    assert!(tracked, "reparented process-group member was not tracked");
    assert_eq!(cleanup_error, None);
    assert!(linux_process_snapshot(pid).is_none_or(|process| process.zombie));
}

#[cfg(target_os = "linux")]
#[test]
fn run_bounded_kills_descendant_process_on_timeout() {
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    let child_pid_path = f.session_dir.join("child.pid");
    make_fake_bin(
        &f.bin_dir,
        "backend",
        &format!(
            "#!/bin/sh\nsh -c 'echo $$ > \"{}\"; sleep 30' &\nwait\n",
            child_pid_path.display()
        ),
    );
    let command = Command::new(f.bin_dir.join("backend"));

    let started = Instant::now();
    let result = run_bounded(command, Duration::from_millis(300));
    assert!(result.is_none());
    assert!(started.elapsed() < Duration::from_secs(5));

    let deadline = Instant::now() + Duration::from_secs(2);
    while !child_pid_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    let child_pid = fs::read_to_string(&child_pid_path)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    while linux_process_snapshot(child_pid).is_some_and(|process| !process.zombie)
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        linux_process_snapshot(child_pid).is_none_or(|process| process.zombie),
        "run_bounded left descendant {child_pid} alive after its timeout, unlike every \
             other timeout path in this file"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn cleanup_kills_descendant_that_escaped_with_setsid() {
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    let escaped_pid_path = f.session_dir.join("escaped.pid");
    make_fake_bin(
        &f.bin_dir,
        "backend",
        &format!(
            "#!/bin/sh\nsetsid sh -c 'echo $$ > \"{}\"; sleep 30' &\nwait\n",
            escaped_pid_path.display()
        ),
    );
    let mut command = Command::new(f.bin_dir.join("backend"));
    command.stdout(Stdio::null()).stderr(Stdio::null());
    prepare_process_group(&mut command);
    let mut child = command.spawn().unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    while !escaped_pid_path.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    let escaped_pid = fs::read_to_string(&escaped_pid_path)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    assert!(linux_process_snapshot(escaped_pid).is_some());

    let cleanup_error = kill_process_group(&mut child);
    let _ = child.wait();
    let deadline = Instant::now() + Duration::from_secs(2);
    while linux_process_snapshot(escaped_pid).is_some_and(|process| !process.zombie)
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        linux_process_snapshot(escaped_pid).is_none_or(|process| process.zombie),
        "setsid descendant {escaped_pid} survived backend cleanup"
    );
    assert_eq!(cleanup_error, None);
}

#[cfg(target_os = "linux")]
#[test]
fn shutdown_watch_kills_escaped_session_and_grandchild() {
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    let escaped_pid_path = f.session_dir.join("escaped-session.pid");
    let grandchild_pid_path = f.session_dir.join("escaped-grandchild.pid");
    make_fake_bin(
        &f.bin_dir,
        "backend",
        &format!(
            "#!/bin/sh\nsetsid sh -c 'echo $$ > \"{}\"; sleep 30 & echo $! > \"{}\"; wait' &\nwait\n",
            escaped_pid_path.display(),
            grandchild_pid_path.display()
        ),
    );
    let shutdown = std::sync::Arc::new(AtomicBool::new(false));
    let trigger = std::sync::Arc::clone(&shutdown);
    let ready_path = escaped_pid_path.clone();
    let ready_grandchild_path = grandchild_pid_path.clone();
    let trigger_thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while (!ready_path.exists() || !ready_grandchild_path.exists()) && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        trigger.store(true, Ordering::SeqCst);
    });
    let log_path = f.session_dir.join("backend-output.log");

    let result = spawn_with_idle_watch_with_shutdown(
        Command::new(f.bin_dir.join("backend")),
        &log_path,
        &f.worktree,
        60,
        "launching escaped-session test backend",
        &shutdown,
        true,
    )
    .unwrap();
    trigger_thread.join().unwrap();

    assert_eq!(result.0, -2);
    for pid_path in [&escaped_pid_path, &grandchild_pid_path] {
        let pid = fs::read_to_string(pid_path)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap();
        assert!(
            linux_process_snapshot(pid).is_none_or(|process| process.zombie),
            "escaped descendant {pid} survived watched shutdown"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn idle_watch_allows_quiet_active_descendant_verification() {
    for attempt in 1..=8 {
        let _exec_guard = ExecGuard::new();
        let f = fixture();
        make_fake_bin(
            &f.bin_dir,
            "backend",
            "#!/bin/sh\n# Deterministic, quiet parent with active descendant.\n/bin/yes >/dev/null &\nbackend_descendant=$!\nsleep 3\n/bin/kill -TERM \"$backend_descendant\" >/dev/null 2>&1 || true\nwait \"$backend_descendant\" 2>/dev/null || true\n",
        );
        let log_path = f.session_dir.join("backend-output.log");
        let record_path = f.session_dir.join("running-worker.json");
        fs::write(&record_path, "{}").unwrap();
        let before = fs::metadata(&record_path).unwrap().modified().unwrap();
        let shutdown = AtomicBool::new(false);

        let result = spawn_with_idle_watch_with_shutdown(
            Command::new(f.bin_dir.join("backend")),
            &log_path,
            &f.worktree,
            2,
            "launching quiet verification backend",
            &shutdown,
            true,
        )
        .unwrap();

        assert!(
            fs::metadata(&record_path).unwrap().modified().unwrap() > before,
            "quiet descendant activity must heartbeat the roster"
        );
        assert_eq!(result.0, 0, "attempt {attempt}");
        assert!(result.1 >= 1.0, "attempt {attempt} ended too quickly");
        let log = fs::read_to_string(log_path).unwrap_or_default();
        assert!(
            !log.contains("GAH: killed after"),
            "attempt {attempt} got log: {log}"
        );
    }
}

/// Issue #116: a backend whose tree burns CPU with a child workload
/// produces measured, non-null, bounded resource observations (the whole
/// tree is sampled, not only the short-lived wrapper PID).
#[cfg(target_os = "linux")]
#[test]
fn idle_watch_reports_measured_resource_usage_for_cpu_workload() {
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    make_fake_bin(
        &f.bin_dir,
        "backend",
        "#!/bin/sh\n/bin/yes >/dev/null &\nworkload=$!\nsleep 2\nkill \"$workload\" 2>/dev/null\nwait 2>/dev/null\n",
    );
    let log_path = f.session_dir.join("backend-resources.log");
    let shutdown = AtomicBool::new(false);
    let (exit_code, duration, resources) = spawn_with_idle_watch_with_shutdown(
        Command::new(f.bin_dir.join("backend")),
        &log_path,
        &f.worktree,
        60,
        "launching resource-sampling test backend",
        &shutdown,
        true,
    )
    .unwrap();
    assert_eq!(exit_code, 0);

    let cpu = resources
        .cpu_time_seconds
        .expect("cpu metric must be present");
    assert_eq!(cpu.quality, crate::ledger::ResourceMetricQuality::Measured);
    let cpu_value = cpu.value.expect("measured cpu is never null");
    assert!(cpu_value > 0.0, "cpu-consuming tree must report cpu > 0");
    assert!(
        cpu_value <= duration + 5.0,
        "cpu {cpu_value}s must be bounded by attempt duration {duration}s (+slack)"
    );

    let rss = resources
        .peak_rss_bytes
        .expect("rss metric must be present");
    assert_eq!(rss.quality, crate::ledger::ResourceMetricQuality::Measured);
    let rss_value = rss.value.expect("measured rss is never null");
    assert!(rss_value > 0.0, "live tree must report rss > 0");
    assert!(
        rss_value < 8.0 * 1024.0 * 1024.0 * 1024.0,
        "rss {rss_value} bytes must be bounded (8 GiB ceiling)"
    );
}

/// Issue #116: an attempt killed for idleness still keeps its observed
/// maxima — provenance stays `measured`, never degraded to unknown.
#[cfg(target_os = "linux")]
#[test]
fn idle_kill_preserves_measured_resource_usage() {
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    make_fake_bin(&f.bin_dir, "backend", "#!/bin/sh\nsleep 30\n");
    let log_path = f.session_dir.join("backend-killed-resources.log");
    let shutdown = AtomicBool::new(false);
    let (exit_code, _duration, resources) = spawn_with_idle_watch_with_shutdown(
        Command::new(f.bin_dir.join("backend")),
        &log_path,
        &f.worktree,
        1,
        "launching killed resource-sampling test backend",
        &shutdown,
        true,
    )
    .unwrap();
    assert_eq!(exit_code, -1);
    let cpu = resources
        .cpu_time_seconds
        .expect("cpu metric must be present even when killed");
    assert_eq!(cpu.quality, crate::ledger::ResourceMetricQuality::Measured);
    let rss = resources
        .peak_rss_bytes
        .expect("rss metric must be present even when killed");
    assert_eq!(rss.quality, crate::ledger::ResourceMetricQuality::Measured);
    assert!(rss.value.is_some_and(|value| value > 0.0));
}

/// Issue #116: on platforms without process-tree measurement the attempt
/// records explicit unsupported provenance — never zero, never absent.
#[cfg(not(target_os = "linux"))]
#[test]
fn idle_watch_reports_unsupported_resources_off_linux() {
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    make_fake_bin(&f.bin_dir, "backend", "#!/bin/sh\nexit 0\n");
    let log_path = f.session_dir.join("backend-unsupported-resources.log");
    let shutdown = AtomicBool::new(false);
    let (exit_code, _duration, resources) = spawn_with_idle_watch_with_shutdown(
        Command::new(f.bin_dir.join("backend")),
        &log_path,
        &f.worktree,
        60,
        "launching unsupported-platform test backend",
        &shutdown,
        true,
    )
    .unwrap();
    assert_eq!(exit_code, 0);
    for metric in [resources.cpu_time_seconds, resources.peak_rss_bytes] {
        let metric = metric.expect("unsupported metric must be present");
        assert_eq!(
            metric.quality,
            crate::ledger::ResourceMetricQuality::Unsupported
        );
        assert!(metric.value.is_none());
        assert!(metric.unknown_reason.is_some());
    }
}

#[test]
fn process_activity_ignores_idle_child_churn_but_detects_real_work() {
    let previous = vec![(10, 1, 0, 20), (11, 0, 0, 0)];

    assert!(!process_group_activity_advanced(
        Some(&previous),
        &[(10, 1, 0, 20), (12, 0, 0, 0)]
    ));
    assert!(process_group_activity_advanced(
        Some(&previous),
        &[(10, 2, 0, 20)]
    ));
    assert!(process_group_activity_advanced(
        Some(&previous),
        &[(13, 1, 0, 0)]
    ));
}

/// Issue #579 regression: a backend (OpenCode-style, worktree-progress
/// watch) that has produced a real diff and is then running a quiet,
/// long-running validation descendant must NOT be classified as no
/// progress and killed. The active validation subprocess is observable
/// progress via descendant (PPID) activity, and the real diff must
/// survive the run.
#[cfg(target_os = "linux")]
#[test]
fn validation_descendant_preserves_diff_past_idle_threshold() {
    for attempt in 1..=8 {
        let _exec_guard = ExecGuard::new();
        let f = fixture();
        initialize_git_worktree(&f.worktree);
        make_fake_bin(
            &f.bin_dir,
            "backend",
            "#!/bin/sh\n\
                 # Produce a real diff, then run a quiet validation descendant\n\
                 # that stays busy past the idle threshold.\n\
                 printf 'pub fn answer() -> u32 { 42 }\\n' > real_diff.rs\n\
                 git add -A >/dev/null 2>&1\n\
                 /bin/yes >/dev/null &\n\
                 backend_descendant=$!\n\
                 sleep 3\n\
                 /bin/kill -TERM \"$backend_descendant\" >/dev/null 2>&1 || true\n\
                 wait \"$backend_descendant\" 2>/dev/null || true\n",
        );
        let log_path = f.session_dir.join("backend-output.log");
        let shutdown = AtomicBool::new(false);
        let mut command = Command::new(f.bin_dir.join("backend"));
        command.current_dir(&f.worktree);

        let result = spawn_with_idle_watch_with_shutdown(
            command,
            &log_path,
            &f.worktree,
            2,
            "launching productive validation backend",
            &shutdown,
            false, // output does NOT count as progress (OpenCode-style)
        )
        .unwrap();

        assert_eq!(result.0, 0, "attempt {attempt}");
        assert!(result.1 >= 1.0, "attempt {attempt} ended too quickly");
        let log = fs::read_to_string(log_path).unwrap_or_default();
        assert!(
            !log.contains("GAH: killed after"),
            "attempt {attempt} got log: {log}"
        );
        // The real diff must remain available after the run.
        assert!(
            f.worktree.join("real_diff.rs").exists(),
            "attempt {attempt}: productive diff was lost"
        );
    }
}

/// Issue #579 regression: terminal attribution distinguishes a backend
/// killed before producing any change from one killed after producing a
/// diff during validation. The kill message carries the matching phrase.
#[cfg(target_os = "linux")]
#[test]
fn idle_kill_attribution_uses_attempt_relative_worktree_progress() {
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    initialize_git_worktree(&f.worktree);
    // No diff produced: a genuinely stalled launch.
    make_fake_bin(&f.bin_dir, "backend", "#!/bin/sh\nsleep 5\n");
    let log_path = f.session_dir.join("backend-output.log");
    let shutdown = AtomicBool::new(false);
    let result = spawn_with_idle_watch_with_shutdown(
        Command::new(f.bin_dir.join("backend")),
        &log_path,
        &f.worktree,
        1,
        "launching stalled backend",
        &shutdown,
        false,
    )
    .unwrap();
    assert_eq!(result.0, -1);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("stalled before changes"), "got: {log}");
    assert!(!log.contains("checkpointed changes"), "got: {log}");

    // A diff that predates the backend attempt is not backend progress.
    drop(_exec_guard);
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    initialize_git_worktree(&f.worktree);
    fs::write(
        f.worktree.join("real_diff.rs"),
        "pub fn inherited() -> u32 { 41 }\n",
    )
    .unwrap();
    make_fake_bin(&f.bin_dir, "backend", "#!/bin/sh\nsleep 5\n");
    let log_path = f.session_dir.join("backend-output.log");
    let shutdown = AtomicBool::new(false);
    let result = spawn_with_idle_watch_with_shutdown(
        Command::new(f.bin_dir.join("backend")),
        &log_path,
        &f.worktree,
        1,
        "launching stalled backend with inherited changes",
        &shutdown,
        false,
    )
    .unwrap();
    assert_eq!(result.0, -1);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("stalled before changes"), "got: {log}");
    assert!(!log.contains("checkpointed changes"), "got: {log}");

    // A change made after launch remains productive checkpointed work.
    drop(_exec_guard);
    let _exec_guard = ExecGuard::new();
    let f = fixture();
    initialize_git_worktree(&f.worktree);
    make_fake_bin(
        &f.bin_dir,
        "backend",
        "#!/bin/sh\nprintf 'pub fn answer() -> u32 { 42 }\\n' > real_diff.rs\ngit add -A >/dev/null 2>&1\nsleep 5\n",
    );
    let log_path = f.session_dir.join("backend-output.log");
    let shutdown = AtomicBool::new(false);
    let mut command = Command::new(f.bin_dir.join("backend"));
    command.current_dir(&f.worktree);
    let result = spawn_with_idle_watch_with_shutdown(
        command,
        &log_path,
        &f.worktree,
        1,
        "launching stalled-but-productive backend",
        &shutdown,
        false,
    )
    .unwrap();
    assert_eq!(result.0, -1);
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(
        log.contains("stalled during validation with checkpointed changes"),
        "got: {log}"
    );
}
