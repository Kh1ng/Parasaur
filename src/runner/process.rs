//! Generic child-process lifecycle and watchdogs, extracted from the runner
//! facade.
//!
//! This module owns the backend-agnostic mechanics of launching a child
//! process group, copying/redacting its streams to a log, snapshotting
//! worktree and descendant CPU/I/O progress, and killing the group on idle
//! stall or graceful shutdown. Backend-specific argv construction, executable
//! resolution, usage/log discovery, and review invocation stay in the facade.

use anyhow::{Context, Result};
#[cfg(target_os = "linux")]
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub(crate) use crate::runner::log_copier::copy_stream_to_file;

/// Set by the loop/dispatch process's SIGINT/SIGTERM handler. Backend runners
/// poll it and terminate their dedicated process group, allowing the caller to
/// write the normal terminal event and ledger record before exiting.
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
pub(crate) const PROCESS_CLEANUP_FAILED_EXIT_CODE: i32 = -3;
/// Internal-only command environment used to carry a per-attempt wall-clock
/// ceiling through backend adapters. The watcher removes it before spawning.
pub(crate) const HARD_TIMEOUT_ENV: &str = "PARASAUR_INTERNAL_HARD_TIMEOUT_SECONDS";

pub fn install_shutdown_handler() -> Result<()> {
    SHUTDOWN_REQUESTED.store(false, Ordering::SeqCst);
    ctrlc::set_handler(|| {
        SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
    })
    .context("installing graceful shutdown handler")
}

/// Runs `cmd` to completion and captures its output, but never waits past
/// `timeout`: on expiry the child is killed and `None` is returned. For
/// bounded self-report queries (e.g. skill inventory, #966) that must never
/// block a caller on a hung backend -- unlike `spawn_with_idle_watch`, there
/// is no idle-vs-progress distinction here, just a hard deadline.
pub fn run_bounded(cmd: Command, timeout: Duration) -> Option<std::process::Output> {
    run_bounded_input(cmd, timeout, None, false)
}

/// Bounded provider-claim command whose descendants must end before its write
/// fence completes. Keep the leader unreaped until group cleanup, and refuse
/// unconfirmed cleanup or more than 1 MiB on either captured stream.
#[cfg(target_os = "linux")]
pub(crate) fn run_bounded_owned(cmd: Command, timeout: Duration) -> Option<std::process::Output> {
    if timeout.is_zero() {
        return None;
    }
    run_bounded_input(cmd, timeout, None, true)
}

/// Same deadline and process-group ownership, with private piped request input.
pub(crate) fn run_bounded_with_input(
    cmd: Command,
    timeout: Duration,
    input: &[u8],
) -> Option<std::process::Output> {
    run_bounded_input(cmd, timeout, Some(input), false)
}

fn run_bounded_input(
    mut cmd: Command,
    timeout: Duration,
    input: Option<&[u8]>,
    owned: bool,
) -> Option<std::process::Output> {
    let mut stdout = tempfile::tempfile().ok()?;
    let mut stderr = tempfile::tempfile().ok()?;
    cmd.stdout(Stdio::from(stdout.try_clone().ok()?))
        .stderr(Stdio::from(stderr.try_clone().ok()?))
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    // Own process group so a timeout can reap the whole tree, not just the
    // direct child -- same discipline as every other timeout path in this
    // file (`kill_process_group`/`kill_process_group_by_pid`); without it a
    // descendant the backend spawns of its own (a shell wrapper, etc.)
    // survives the SIGKILL below and leaks.
    prepare_process_group(&mut cmd);
    let mut child = cmd.spawn().ok()?;
    if let Some(input) = input {
        let mut stdin = child.stdin.take()?;
        let input = input.to_vec();
        thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let deadline = Instant::now() + timeout;
    loop {
        let observed = if owned {
            #[cfg(target_os = "linux")]
            {
                if crate::runner::exited_without_reaping(&child).ok()? {
                    let cleanup = kill_process_group(&mut child);
                    let status = child.wait().ok()?;
                    if cleanup.is_some() {
                        return None;
                    }
                    Ok(Some(status))
                } else {
                    Ok(None)
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                child.try_wait()
            }
        } else {
            child.try_wait()
        };
        match observed {
            Ok(Some(status)) => {
                use std::io::Seek;
                stdout.rewind().ok()?;
                stderr.rewind().ok()?;
                let mut stdout_bytes = Vec::new();
                let mut stderr_bytes = Vec::new();
                let limit = if owned { 1024 * 1024 } else { u64::MAX - 1 };
                stdout.take(limit + 1).read_to_end(&mut stdout_bytes).ok()?;
                stderr.take(limit + 1).read_to_end(&mut stderr_bytes).ok()?;
                if stdout_bytes.len() as u64 > limit || stderr_bytes.len() as u64 > limit {
                    return None;
                }
                return Some(std::process::Output {
                    status,
                    stdout: stdout_bytes,
                    stderr: stderr_bytes,
                });
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Ok(None) => {
                kill_process_group(&mut child);
                let _ = child.wait();
                return None;
            }
            Err(_) => {
                kill_process_group(&mut child);
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(all(test, unix))]
#[test]
fn bounded_private_stdin_cannot_block_the_process_deadline() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "sleep 60"]);
    let start = Instant::now();
    assert!(run_bounded_with_input(
        command,
        Duration::from_millis(100),
        &vec![b'x'; 1024 * 1024]
    )
    .is_none());
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[cfg(all(test, target_os = "linux"))]
#[test]
fn native_claim_command_reaps_descendants_and_bounds_capture() {
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("child");
    let mut expired = Command::new("/usr/bin/touch");
    expired.arg(&marker);
    assert!(run_bounded_owned(expired, Duration::ZERO).is_none());
    assert!(!marker.exists());
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "sleep 60 & echo $! > \"$1\"; exit 0", "fixture"]);
    command.arg(&marker);
    assert!(run_bounded_owned(command, Duration::from_secs(2))
        .unwrap()
        .status
        .success());
    let pid: u32 = fs::read_to_string(marker).unwrap().trim().parse().unwrap();
    assert!(linux_process_snapshot(pid).is_none_or(|process| process.zombie));
    let mut oversized = Command::new("/usr/bin/head");
    oversized.args(["-c", "1048577", "/dev/zero"]);
    assert!(run_bounded_owned(oversized, Duration::from_secs(2)).is_none());
}

pub fn shutdown_requested() -> bool {
    SHUTDOWN_REQUESTED.load(Ordering::SeqCst)
}

#[cfg(unix)]
pub(crate) fn prepare_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
pub(crate) fn prepare_process_group(_cmd: &mut Command) {}

/// Arms `PR_SET_PDEATHSIG` on the child so the kernel kills it directly if
/// gah's own process dies for any reason (crash, SIGKILL, cascading parent
/// death) -- independent of whether any of gah's own Rust code is still
/// alive to explicitly reap it. Every real backend invocation is instead
/// explicitly tracked and killed via `kill_process_group` from a live
/// idle-watch loop, which is enough on its own *because* that loop's owning
/// process (the recurring `gah loop` daemon) is itself the one armed via
/// `controller::ownership::arm_parent_death_signal` -- it survives long
/// enough after its own launcher dies to gracefully clean up its tracked
/// children before exiting. This function is for the opposite shape: a
/// short-lived, fire-and-forget child (issue #761's background quota
/// probe) spawned from a detached thread that nothing explicitly joins or
/// polls for shutdown -- if gah's process exits while that thread is still
/// waiting on its child, the child would otherwise be silently orphaned
/// with no supervising code left running to kill it. Same primitive
/// `arm_parent_death_signal` uses on gah's own process, applied to a child
/// instead via `pre_exec`, same pattern as `prepare_process_group` above.
#[cfg(target_os = "linux")]
pub(crate) fn arm_child_pdeathsig(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn arm_child_pdeathsig(_cmd: &mut Command) {}

pub(crate) fn kill_process_group(child: &mut std::process::Child) -> Option<String> {
    #[cfg(not(target_os = "linux"))]
    let cleanup_error = None;
    #[cfg(target_os = "linux")]
    let cleanup_error = {
        let survivors = kill_linux_process_tree(child.id());
        if survivors.is_empty() {
            None
        } else {
            Some(format!(
                "backend descendants still alive after SIGKILL: {survivors:?}"
            ))
        }
    };
    #[cfg(all(unix, not(target_os = "linux")))]
    unsafe {
        let _ = libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    cleanup_error
}

/// Issue #761: `kill_process_group` needs a live `&mut Child` -- fine for
/// every real backend invocation, which is always driven by a polling loop
/// that owns the `Child` for its whole lifetime. The background quota
/// probe (`quota_store::refresh_stale_quota_observations`) doesn't fit
/// that shape: it runs on a detached thread nothing joins, so by the time
/// `gah loop`'s shutdown path notices it should clean up, the `Child`
/// value itself lives on a thread stack that path has no access to. This
/// PID-only variant is what `SUPERVISED_CHILDREN` (below) lets that
/// shutdown path use instead -- same underlying tree-walk-and-kill
/// (`kill_linux_process_tree`), just addressed by PID rather than an owned
/// handle. Confirmed this tree-walk is load-bearing, not redundant with
/// `arm_child_pdeathsig`: `PR_SET_PDEATHSIG` only fires for the direct
/// child, and does not cascade to grandchildren a shell-script-shaped
/// child spawns of its own (observed live via
/// `tests/recurring_loop_parent_death.rs`'s fake-binary fixture, which
/// spawns exactly such a grandchild) -- PDEATHSIG alone left one alive.
pub(crate) fn kill_process_group_by_pid(pid: u32) {
    #[cfg(target_os = "linux")]
    {
        kill_linux_process_tree(pid);
    }
    #[cfg(not(target_os = "linux"))]
    unsafe {
        let _ = libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

/// Registry of PIDs a live-but-unjoined background thread is currently
/// supervising, so `gah loop`'s shutdown path (which owns none of those
/// threads or their `Child` handles) can still explicitly kill them before
/// the process exits -- see `kill_process_group_by_pid`'s doc comment for
/// why this is necessary at all rather than relying on
/// `arm_child_pdeathsig` alone.
static SUPERVISED_CHILD_PIDS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn register_supervised_child(pid: u32) {
    if let Ok(mut pids) = SUPERVISED_CHILD_PIDS.lock() {
        pids.push(pid);
    }
}

pub(crate) fn unregister_supervised_child(pid: u32) {
    if let Ok(mut pids) = SUPERVISED_CHILD_PIDS.lock() {
        pids.retain(|&p| p != pid);
    }
}

/// Called from `gah loop`'s own shutdown path once it detects
/// `shutdown_requested()`, before it actually returns/exits -- the last
/// point at which any of this process's own code is guaranteed to still
/// be running to do the kill.
///
/// Polls for a short bounded window rather than checking once: a probe
/// thread can be mid-flight between `std::thread::spawn` and its own
/// `register_supervised_child` call (it hasn't reached `Command::spawn()`
/// yet) at the exact moment shutdown is detected. A single snapshot-and-kill
/// would see nothing registered and let that child orphan the instant it's
/// actually spawned a moment later, with the whole process exiting before
/// any Rust code -- this function included -- runs again. Bounded short:
/// this delays every shutdown by up to this long, not just the rare
/// in-flight-probe case.
pub(crate) fn kill_all_supervised_children() {
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        let pids = SUPERVISED_CHILD_PIDS
            .lock()
            .map(|pids| pids.clone())
            .unwrap_or_default();
        for &pid in &pids {
            kill_process_group_by_pid(pid);
            unregister_supervised_child(pid);
        }
        if Instant::now() >= deadline {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ProcessIdentity {
    pub(crate) pid: u32,
    pub(crate) start_ticks: u64,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProcessSnapshot {
    pub(crate) identity: ProcessIdentity,
    pub(crate) parent_pid: u32,
    pub(crate) process_group: u32,
    pub(crate) zombie: bool,
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_process_snapshot(pid: u32) -> Option<ProcessSnapshot> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields = stat[stat.rfind(") ")? + 2..]
        .split_whitespace()
        .collect::<Vec<_>>();
    Some(ProcessSnapshot {
        identity: ProcessIdentity {
            pid,
            start_ticks: fields.get(19)?.parse().ok()?,
        },
        parent_pid: fields.get(1)?.parse().ok()?,
        process_group: fields.get(2)?.parse().ok()?,
        zombie: fields.first().is_some_and(|state| *state == "Z"),
    })
}

/// Snapshot live descendants by both PPID and membership in the root's group.
/// PPID finds tools that escaped with `setsid`; group membership retains
/// children reparented after their leader exits, before cleanup can run.
#[cfg(target_os = "linux")]
pub(crate) fn linux_descendants(root_pid: u32) -> Vec<ProcessIdentity> {
    let snapshots = fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
        .filter_map(linux_process_snapshot)
        .filter(|snapshot| !snapshot.zombie)
        .collect::<Vec<_>>();
    let mut ancestors = HashSet::from([root_pid]);
    let mut descendants = Vec::new();
    loop {
        let mut added = false;
        for snapshot in &snapshots {
            if (ancestors.contains(&snapshot.parent_pid) || snapshot.process_group == root_pid)
                && ancestors.insert(snapshot.identity.pid)
            {
                descendants.push(snapshot.identity);
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    descendants
}

#[cfg(target_os = "linux")]
fn signal_process(identity: ProcessIdentity, signal: libc::c_int) {
    let Some(current) = linux_process_snapshot(identity.pid) else {
        return;
    };
    if current.identity != identity || current.zombie {
        return;
    }
    unsafe {
        let _ = libc::kill(identity.pid as libc::pid_t, signal);
    }
}

#[cfg(target_os = "linux")]
fn kill_linux_process_tree(root_pid: u32) -> Vec<u32> {
    let mut descendants = linux_descendants(root_pid);
    // Freeze the original group first so the backend cannot race cleanup by
    // spawning more children. Freeze escaped descendants by exact PID, then
    // rescan once to catch a fork that landed between the first scan/signals.
    unsafe {
        let _ = libc::kill(-(root_pid as libc::pid_t), libc::SIGSTOP);
    }
    for identity in &descendants {
        signal_process(*identity, libc::SIGSTOP);
    }
    // Reach a fixed point. A descendant can fork between the first snapshot
    // and receiving SIGSTOP; each newly discovered process is frozen before
    // the next scan, so eventually no runnable member of the tree remains.
    loop {
        let mut added = false;
        for identity in linux_descendants(root_pid) {
            if !descendants.contains(&identity) {
                signal_process(identity, libc::SIGSTOP);
                descendants.push(identity);
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    unsafe {
        let _ = libc::kill(-(root_pid as libc::pid_t), libc::SIGKILL);
    }
    for identity in descendants.iter().rev() {
        signal_process(*identity, libc::SIGKILL);
    }
    let deadline = Instant::now() + Duration::from_millis(250);
    loop {
        let survivors = descendants
            .iter()
            .filter_map(|identity| {
                linux_process_snapshot(identity.pid)
                    .filter(|process| process.identity == *identity && !process.zombie)
                    .map(|process| process.identity.pid)
            })
            .collect::<Vec<_>>();
        if survivors.is_empty() || Instant::now() >= deadline {
            return survivors;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn write_redacted_task(session_dir: &Path, task: &str) -> Result<()> {
    fs::write(session_dir.join("task.md"), crate::redact::redact(task))
        .context("writing redacted task artifact")
}

/// Return a content-sensitive snapshot of a worktree's tracked changes.
///
/// Several subscription CLIs perform tool calls without forwarding their
/// progress to stdout. Treating that silence as a hang kills an agent that is
/// still editing source. `git diff` makes those edits observable without
/// walking ignored build output such as `target/` or `node_modules/`.
pub(crate) fn worktree_progress_snapshot(worktree: &Path) -> Option<Vec<u8>> {
    let diff = Command::new("git")
        .args(["diff", "--no-ext-diff", "--binary", "HEAD", "--"])
        .current_dir(worktree)
        .output()
        .ok()?;
    if !diff.status.success() {
        return None;
    }
    let staged = Command::new("git")
        .args([
            "diff",
            "--cached",
            "--no-ext-diff",
            "--binary",
            "HEAD",
            "--",
        ])
        .current_dir(worktree)
        .output()
        .ok()?;
    if !staged.status.success() {
        return None;
    }
    let status = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=all"])
        .current_dir(worktree)
        .output()
        .ok()?;
    if !status.status.success() {
        return None;
    }

    let mut snapshot = diff.stdout;
    snapshot.extend_from_slice(&staged.stdout);
    snapshot.extend_from_slice(&status.stdout);
    Some(snapshot)
}

/// Did this backend attempt leave durable worktree progress relative to the
/// exact state it inherited? Existing PR changes and target-merge conflicts
/// must not make an otherwise idle backend look productive.
fn worktree_changed_since(initial: Option<&[u8]>, worktree: &Path) -> bool {
    let Some(initial) = initial else {
        return false;
    };
    worktree_progress_snapshot(worktree).is_some_and(|current| current != initial)
}

/// Linux activity snapshot of every live descendant of `process_group` (by
/// process-tree parentage, not just process-group membership) that is
/// meaningful even when a backend is quiet and its source tree is stable.
///
/// Build and test commands commonly run for minutes without writing either
/// stream or touching tracked files. CPU or I/O activity proves that the
/// backend is still executing; mere process existence or idle child churn
/// does not, so a sleeping/hung backend expires.
///
/// Issue #579: a validation subprocess (e.g. `cargo test`) launched with
/// `setsid` deliberately escapes the backend process group while remaining in
/// its process tree. Matching only on `pgrp` would miss it, causing GAH to
/// classify a backend that is actively running the required tests as
/// "no progress" and kill it. We therefore follow parent links as well as
/// group membership, so escaped verification descendants still count. The backend
/// root itself is excluded: its activity is already represented by
/// output/worktree progress, and counting a spinning root would mask a hang.
#[cfg(target_os = "linux")]
pub(crate) fn process_group_activity_snapshot(
    process_group: u32,
) -> Option<Vec<(u32, u64, u64, u64)>> {
    let mut members = Vec::new();
    for identity in linux_descendants(process_group) {
        // The root backend process is excluded; see doc above.
        if identity.pid == process_group {
            continue;
        }
        let path = format!("/proc/{}/stat", identity.pid);
        let stat = fs::read_to_string(&path).ok();
        let Some(stat) = stat else { continue };
        // `comm` is parenthesized and may contain spaces or parentheses, so
        // split only after its final closing delimiter. The remaining fields
        // start at field 3 (`state`).
        let Some(fields) = stat
            .rfind(") ")
            .map(|end| stat[end + 2..].split_whitespace().collect::<Vec<_>>())
        else {
            continue;
        };
        let user_ticks = fields
            .get(11)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let system_ticks = fields
            .get(12)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        let io_bytes = fs::read_to_string(format!("/proc/{}/io", identity.pid))
            .ok()
            .map(|io| {
                io.lines()
                    .filter_map(|line| line.split_once(':'))
                    .filter(|(name, _)| {
                        matches!(
                            name.trim(),
                            "rchar" | "wchar" | "read_bytes" | "write_bytes"
                        )
                    })
                    .filter_map(|(_, value)| value.trim().parse::<u64>().ok())
                    .sum()
            })
            .unwrap_or(0);
        members.push((identity.pid, user_ticks, system_ticks, io_bytes));
    }
    members.sort_unstable();
    Some(members)
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn process_group_activity_snapshot(
    _process_group: u32,
) -> Option<Vec<(u32, u64, u64, u64)>> {
    None
}

pub(crate) fn process_group_activity_advanced(
    previous: Option<&[(u32, u64, u64, u64)]>,
    current: &[(u32, u64, u64, u64)],
) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    current.iter().any(|&(pid, user, system, io)| {
        previous
            .iter()
            .find(|&&(previous_pid, _, _, _)| previous_pid == pid)
            .map(|&(_, previous_user, previous_system, previous_io)| {
                user > previous_user || system > previous_system || io > previous_io
            })
            // Process creation alone is not progress: a stalled shell can
            // churn `sleep` children forever. A new descendant only counts
            // after it has consumed measurable CPU, as real compilers and
            // test processes do.
            .unwrap_or(user > 0 || system > 0)
    })
}

/// Spawn `cmd` (stdout/stderr are set to piped by this helper) and drive it
/// to completion, killing it only once both its log and worktree have gone
/// genuinely quiet for `idle_timeout_seconds` -- never on a flat wall-clock
/// budget. Shared by every backend invocation that needs hang protection
/// (agy, opencode, openhands, vibe, codex, claude) -- extracted after the
/// third copy-paste of this exact loop (issues #170/#87) made the
/// duplication no longer defensible.
///
/// `spawn_context` labels a spawn failure (e.g. "launching vibe; is it
/// installed and on PATH?"). Returns `(exit_code, duration_secs)`; on an
/// idle kill, exit_code is -1; failed descendant cleanup uses -3. Both append
/// an explicit diagnostic to the log.
/// Spawn `cmd` and run it under the idle watch, returning
/// `(exit_code, duration_secs, resource_usage)` -- see the inner function
/// for the resource-usage provenance (#116).
pub(crate) fn spawn_with_idle_watch(
    cmd: Command,
    log_path: &Path,
    worktree: &Path,
    idle_timeout_seconds: u64,
    spawn_context: &str,
) -> Result<(i32, f64, crate::ledger::AttemptResourceUsage)> {
    spawn_with_idle_watch_with_shutdown(
        cmd,
        log_path,
        worktree,
        idle_timeout_seconds,
        spawn_context,
        &SHUTDOWN_REQUESTED,
        true,
    )
}

/// Run a backend under a semantic-progress watch. Unlike the generic idle
/// watch, arbitrary stdout/stderr does not reset this window: only a durable
/// worktree change does. This is for CLIs such as OpenCode that can stream
/// malformed tool-call chatter indefinitely without actually executing work.
pub(crate) fn spawn_with_worktree_progress_watch(
    cmd: Command,
    log_path: &Path,
    worktree: &Path,
    idle_timeout_seconds: u64,
    spawn_context: &str,
) -> Result<(i32, f64, crate::ledger::AttemptResourceUsage)> {
    spawn_with_idle_watch_with_shutdown(
        cmd,
        log_path,
        worktree,
        idle_timeout_seconds,
        spawn_context,
        &SHUTDOWN_REQUESTED,
        false,
    )
}

fn spawn_with_idle_watch_with_shutdown(
    mut cmd: Command,
    log_path: &Path,
    worktree: &Path,
    idle_timeout_seconds: u64,
    spawn_context: &str,
    shutdown_requested: &AtomicBool,
    output_counts_as_progress: bool,
) -> Result<(i32, f64, crate::ledger::AttemptResourceUsage)> {
    // Pessimistic durable evidence: even a full disk that prevents the final
    // notice cannot make an earlier assistant message look authoritative.
    let capture_marker =
        log_path.with_extension(crate::runner::log_copier::INCOMPLETE_CAPTURE_EXTENSION);
    let prior_incomplete = match fs::symlink_metadata(&capture_marker) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    if !prior_incomplete {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&capture_marker)
            .context("marking backend output capture incomplete")?;
    }
    let start = Instant::now();
    let hard_timeout = cmd
        .get_envs()
        .find_map(|(key, value)| {
            (key == HARD_TIMEOUT_ENV)
                .then(|| value.and_then(|value| value.to_str()?.parse::<u64>().ok()))
                .flatten()
        })
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs);
    let hard_deadline = hard_timeout.map(crate::clock::Deadline::after);
    cmd.env_remove(HARD_TIMEOUT_ENV);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    prepare_process_group(&mut cmd);

    // Capture the inherited state before spawn. A fast backend can write its
    // first edit immediately; sampling after spawn would absorb that real
    // change into the baseline and later misclassify it as no progress.
    let initial_worktree_snapshot = worktree_progress_snapshot(worktree);
    // Test time is scoped to the process unless a harness opts a child in.
    cmd.env_remove("PARASAUR_TEST_NOW");
    let mut child = cmd.spawn().with_context(|| spawn_context.to_string())?;
    // Issue #116: start sampling the backend's whole process tree immediately
    // after spawn. Sampling runs for the whole attempt -- success, idle kill,
    // hard timeout, or shutdown -- so killed/cancelled attempts keep their
    // final observed maxima. `finish_resource_sampler` below stops it.
    #[cfg(target_os = "linux")]
    let (resource_stop, resource_accumulator, resource_handle) =
        crate::runner::resources::spawn_resource_sampler(child.id());
    let (progress_tx, progress_rx) = mpsc::sync_channel(1);
    let stdout_thread = child.stdout.take().map(|stdout| {
        copy_stream_to_file(stdout, log_path.to_path_buf(), Some(progress_tx.clone()))
    });
    let stderr_thread = child
        .stderr
        .take()
        .map(|stderr| copy_stream_to_file(stderr, log_path.to_path_buf(), Some(progress_tx)));

    let idle_timeout = Duration::from_secs(idle_timeout_seconds);
    // A backend that emits no output is already indistinguishable from a
    // stalled launch after one idle window. Do not double the stall budget:
    // that kept three workers occupied for ten minutes before the controller
    // could reroute them.
    let startup_grace = idle_timeout;
    let startup_deadline = crate::clock::Deadline::after(startup_grace);
    let poll_interval = Duration::from_millis(500);
    let worktree_poll_interval = Duration::from_secs(1);
    let mut last_seen_len = fs::metadata(log_path).map(|m| m.len()).unwrap_or(0);
    let mut last_worktree_snapshot = initial_worktree_snapshot.clone();
    let process_group = child.id();
    let mut last_process_activity = process_group_activity_snapshot(process_group);
    let mut last_worktree_poll = Instant::now();
    let mut last_progress_at = Instant::now();
    let mut idle_deadline = crate::clock::Deadline::after(idle_timeout);
    let mut saw_progress = false;
    let mut had_attempt_progress_at_kill = false;
    let mut killed_for_idle = false;
    let mut killed_for_hard_timeout = false;
    let mut killed_for_shutdown = false;
    let mut cleanup_error = None;
    let mut owned = true;
    // `None` once the backend exited on its own. The leader stays unreaped
    // until cleanup below has signalled its group, so that group id cannot
    // name an unrelated group by then.
    let killed_exit_code = loop {
        match crate::runner::exited_without_reaping(&child) {
            Ok(true) => break None,
            Ok(false) => {
                if shutdown_requested.load(Ordering::SeqCst) {
                    cleanup_error = kill_process_group(&mut child);
                    killed_for_shutdown = true;
                    break Some(-2);
                }
                let previous_progress_at = last_progress_at;
                if progress_rx.try_recv().is_ok() {
                    let current_len = fs::metadata(log_path)
                        .map(|m| m.len())
                        .unwrap_or(last_seen_len);
                    if output_counts_as_progress {
                        last_progress_at = Instant::now();
                        idle_deadline = crate::clock::Deadline::after(idle_timeout);
                        saw_progress = true;
                    }
                    last_seen_len = current_len;
                }
                let current_len = fs::metadata(log_path)
                    .map(|m| m.len())
                    .unwrap_or(last_seen_len);
                if current_len != last_seen_len {
                    last_seen_len = current_len;
                    if output_counts_as_progress {
                        last_progress_at = Instant::now();
                        idle_deadline = crate::clock::Deadline::after(idle_timeout);
                        saw_progress = true;
                    }
                }
                if last_worktree_poll.elapsed() >= worktree_poll_interval {
                    if let Some(snapshot) = worktree_progress_snapshot(worktree) {
                        if last_worktree_snapshot.as_ref() != Some(&snapshot) {
                            last_worktree_snapshot = Some(snapshot);
                            last_progress_at = Instant::now();
                            idle_deadline = crate::clock::Deadline::after(idle_timeout);
                            saw_progress = true;
                        }
                    }
                    if let Some(activity) = process_group_activity_snapshot(process_group) {
                        if process_group_activity_advanced(
                            last_process_activity.as_deref(),
                            &activity,
                        ) {
                            last_progress_at = Instant::now();
                            idle_deadline = crate::clock::Deadline::after(idle_timeout);
                            saw_progress = true;
                        }
                        last_process_activity = Some(activity);
                    }
                    last_worktree_poll = Instant::now();
                }
                if last_progress_at != previous_progress_at {
                    if let Some(session) = log_path.parent() {
                        crate::running_workers::heartbeat(session);
                    }
                }
                let stalled = if saw_progress {
                    idle_deadline.reached()
                } else {
                    startup_deadline.reached()
                };
                if hard_deadline.is_some_and(crate::clock::Deadline::reached) {
                    cleanup_error = kill_process_group(&mut child);
                    killed_for_hard_timeout = true;
                    break Some(-1);
                }
                if stalled {
                    // Issue #579: record whether the backend had already
                    // produced a repository diff when it stalled. A backend
                    // killed *before* any change is a genuine agent no-progress
                    // signal; one killed *during* validation after producing a
                    // diff is a productive worker whose long build/test run was
                    // mistaken for a hang. The two must be attributed
                    // differently and the latter's diff must be preserved.
                    had_attempt_progress_at_kill =
                        worktree_changed_since(initial_worktree_snapshot.as_deref(), worktree);
                    cleanup_error = kill_process_group(&mut child);
                    killed_for_idle = true;
                    break Some(-1);
                }
                let _ = crate::clock::wait_for(poll_interval, || {
                    shutdown_requested.load(Ordering::SeqCst)
                });
            }
            Err(error) => {
                // Not provably ours any more (ECHILD: already reaped), so its
                // pid and group id may name other processes: no signal, no wait.
                cleanup_error = Some(format!("lost track of backend process: {error}"));
                owned = false;
                break Some(-1);
            }
        }
    };
    let duration = start.elapsed();
    // Stop the resource sampler first so its final scan lands after the last
    // tree state the attempt produced, then take its totals.
    let resource_sample = {
        #[cfg(target_os = "linux")]
        {
            crate::runner::resources::finish_resource_sampler(
                resource_stop,
                resource_accumulator,
                resource_handle,
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            crate::runner::resources::finish_resource_sampler_none()
        }
    };
    // The attempt is over, so whatever the backend left in its process group
    // ends with it. A surviving descendant may still own the inherited pipe
    // descriptors (one outside the group is not reachable here); wait for the
    // log copiers only for a bounded grace, never an unbounded join.
    let status = owned.then(|| {
        crate::runner::kill_unreaped_process_group(&child);
        child.wait()
    });
    let mut exit_code = killed_exit_code.unwrap_or_else(|| {
        let code = status.and_then(Result::ok).and_then(|status| status.code());
        code.unwrap_or(-1)
    });
    let output_complete =
        crate::runner::pipes::readers_finish_within_grace(&[&stdout_thread, &stderr_thread]);
    if !output_complete {
        if let Ok(mut file) = fs::OpenOptions::new().append(true).open(log_path) {
            let _ = writeln!(
                file,
                "GAH: backend output was still held open by a process outside its process group; log may be incomplete."
            );
        }
    }
    // Stop collecting: a copier still reading closes its pipe at its next read.
    drop(progress_rx);
    let capture_complete = output_complete
        && [stdout_thread, stderr_thread]
            .into_iter()
            .flatten()
            .all(|reader| reader.join().unwrap_or(false));
    if capture_complete && !prior_incomplete {
        let _ = fs::remove_file(&capture_marker);
    }
    if let Some(error) = cleanup_error {
        exit_code = PROCESS_CLEANUP_FAILED_EXIT_CODE;
        if let Ok(mut file) = fs::OpenOptions::new().append(true).open(log_path) {
            let _ = writeln!(file, "GAH: harness process cleanup failed: {error}");
        }
    }

    if killed_for_idle {
        if let Ok(mut file) = fs::OpenOptions::new().append(true).open(log_path) {
            let progress_description = if output_counts_as_progress {
                "backend output or worktree progress"
            } else {
                "worktree progress"
            };
            // Issue #579: terminal attribution distinguishes a backend that
            // stalled before producing any repository change from one that
            // stalled during validation after producing a diff. The latter
            // phrase signals the dispatch layer to checkpoint (preserve) the
            // WIP diff instead of discarding it.
            let attribution = if had_attempt_progress_at_kill {
                "stalled during validation with checkpointed changes"
            } else {
                "stalled before changes"
            };
            let _ = writeln!(
                file,
                "GAH: killed after {idle_timeout_seconds}s with no new {progress_description} ({attribution}, not just slow)."
            );
        }
    }
    if killed_for_hard_timeout {
        if let Ok(mut file) = fs::OpenOptions::new().append(true).open(log_path) {
            let seconds = hard_timeout
                .map(|timeout| timeout.as_secs())
                .unwrap_or_default();
            let _ = writeln!(
                file,
                "GAH: killed after reaching the configured {seconds}s hard wall-clock timeout."
            );
        }
    }
    if killed_for_shutdown {
        if let Ok(mut file) = fs::OpenOptions::new().append(true).open(log_path) {
            let _ = writeln!(
                file,
                "GAH: shutdown requested; terminated backend process group."
            );
        }
    }

    Ok((exit_code, duration.as_secs_f64(), resource_sample))
}

#[cfg(test)]
mod tests;
