//! Bounded collection of a child's output pipes.
//!
//! A child's stdout/stderr reach EOF only once every process holding the
//! write ends has closed them. Descendants inherit those descriptors, so a
//! backgrounded helper keeps them open after the direct child has exited.
//! Callers therefore terminate leftover members of the child's process group
//! and wait for EOF for at most a short grace period; output read before the
//! deadline is kept either way.

use std::io::Read;
use std::process::Child;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// How long a caller waits for output pipes to reach EOF after the process
/// group that owned them has exited or been killed.
pub(crate) const PIPE_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// SIGKILL every process still in the process group `leader` leads.
///
/// Call only before `leader` is reaped (observe its exit with
/// `exited_without_reaping`, signal, then `wait`). An unreaped leader keeps
/// its pid, and so the group id, reserved; once it is reaped and the group
/// empties, the number can name an unrelated group.
pub(crate) fn kill_unreaped_process_group(leader: &Child) {
    unsafe {
        let _ = libc::kill(-(leader.id() as libc::pid_t), libc::SIGKILL);
    }
}

/// True once `child` has exited, without reaping it (see
/// `kill_unreaped_process_group`).
///
/// An error means the child can no longer be proven ours: ECHILD says it was
/// already reaped (an inherited `SIGCHLD=SIG_IGN` reaps every child as it
/// exits), so its pid and group id may already name unrelated processes.
/// After an error, never signal or wait on the child; report the lost
/// cleanup instead.
pub(crate) fn exited_without_reaping(child: &Child) -> std::io::Result<bool> {
    loop {
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == -1 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        return Ok(unsafe { info.assume_init().si_pid() } != 0);
    }
}

/// Make exited children wait to be reaped: turn an inherited `SIGCHLD`
/// `SIG_IGN` back into `SIG_DFL` and clear `SA_NOCLDWAIT`, keeping any
/// installed handler.
///
/// Either setting lets the kernel reap a child the moment it exits, even
/// after `exited_without_reaping` saw it still running, so its pid and group
/// id can be reused before a later group signal. The process-group cleanup
/// here relies on this not happening. `cli::run` calls this first, before any
/// thread or child exists; a library caller that manages children through
/// this module must call it (or never ignore `SIGCHLD`) the same way. The
/// lost-ownership guards still catch a child reaped by something else.
pub(crate) fn keep_exited_children_waitable() -> std::io::Result<()> {
    unsafe {
        let mut action = std::mem::zeroed::<libc::sigaction>();
        if libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) == -1 {
            return Err(std::io::Error::last_os_error());
        }
        if action.sa_sigaction != libc::SIG_IGN && action.sa_flags & libc::SA_NOCLDWAIT == 0 {
            return Ok(());
        }
        if action.sa_sigaction == libc::SIG_IGN {
            action.sa_sigaction = libc::SIG_DFL;
        }
        action.sa_flags &= !libc::SA_NOCLDWAIT;
        if libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) == -1 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Wait at most `PIPE_DRAIN_GRACE` for every reader thread to finish.
pub(crate) fn readers_finish_within_grace<T>(readers: &[&Option<JoinHandle<T>>]) -> bool {
    wait_until(Instant::now() + PIPE_DRAIN_GRACE, || {
        readers
            .iter()
            .all(|reader| reader.as_ref().is_none_or(JoinHandle::is_finished))
    })
}

/// Poll `done` until it holds or `deadline` passes; returns whether it held.
pub(crate) fn wait_until(deadline: Instant, done: impl Fn() -> bool) -> bool {
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// A pipe drained on its own thread into a shared buffer, so a caller can
/// stop waiting at a deadline and still keep the bytes read so far. Dropping
/// it without `take` (an early error return, a panic) abandons the pipe the
/// same way `take` does.
pub(crate) struct PipeCapture {
    /// `None` once the capture was taken or dropped.
    bytes: Arc<Mutex<Option<Vec<u8>>>>,
    /// `None` only inside `take`, after it joined a finished reader.
    reader: Option<JoinHandle<std::io::Result<()>>>,
}

impl PipeCapture {
    pub(crate) fn spawn<R: Read + Send + 'static>(mut pipe: R) -> Self {
        let bytes = Arc::new(Mutex::new(Some(Vec::new())));
        let sink = Arc::clone(&bytes);
        let reader = thread::spawn(move || {
            let mut buf = [0_u8; 8192];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => return Ok(()),
                    Ok(read) => {
                        let mut sink = sink.lock().unwrap_or_else(PoisonError::into_inner);
                        // Abandoned: stop retaining and close the read end,
                        // so a writer outside the group gets EPIPE.
                        let Some(bytes) = sink.as_mut() else {
                            return Ok(());
                        };
                        bytes.extend_from_slice(&buf[..read]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
        });
        Self {
            bytes,
            reader: Some(reader),
        }
    }

    /// True once the pipe reached EOF (or the read failed).
    pub(crate) fn is_finished(&self) -> bool {
        self.reader.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Return the bytes read so far and whether the pipe reached EOF. A
    /// still-open pipe is abandoned: its reader keeps nothing more and exits
    /// at its next read, or once the last holder closes the pipe.
    // ponytail: a silent holder parks the reader (one thread + fd, no growth)
    // until it writes or exits; poll(2) with a cancel flag if that matters.
    pub(crate) fn take(mut self) -> std::io::Result<(Vec<u8>, bool)> {
        // Check before taking the bytes: a reader that finishes in between
        // may have dropped a final chunk, and must not be reported as EOF.
        let finished = self.is_finished();
        if finished {
            if let Some(reader) = self.reader.take() {
                reader
                    .join()
                    .map_err(|_| std::io::Error::other("pipe reader panicked"))??;
            }
        }
        Ok((self.abandon(), finished))
    }

    /// Move out the bytes read so far; the reader keeps nothing more.
    fn abandon(&self) -> Vec<u8> {
        self.bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or_default()
    }
}

impl Drop for PipeCapture {
    fn drop(&mut self) {
        self.abandon();
    }
}

/// Test helper: wait up to `within` for `pid` to be gone. A killed orphan
/// may linger as a zombie when the container's init does not reap promptly;
/// a zombie no longer runs or holds descriptors, so it counts as gone.
/// Test helper: the pid a fixture script wrote to `pid_file`.
#[cfg(all(test, unix))]
pub(crate) fn read_pid(pid_file: &std::path::Path) -> u32 {
    let text = std::fs::read_to_string(pid_file).unwrap();
    text.trim().parse().unwrap()
}

#[cfg(all(test, unix))]
pub(crate) fn process_exits_within(pid: u32, within: Duration) -> bool {
    wait_until(Instant::now() + within, || {
        if unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
            return true;
        }
        std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().starts_with('Z'))
            .unwrap_or(false)
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};

    #[test]
    fn capture_keeps_partial_output_of_a_pipe_held_open_by_an_escaped_process() {
        // `setsid` would be the real-world escape; a plain background child
        // outside any killed group is enough to hold the write end here.
        let mut child = Command::new("sh")
            .args(["-c", "printf early; sleep 5 & exit 0"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let capture = PipeCapture::spawn(child.stdout.take().unwrap());
        child.wait().unwrap();

        let started = Instant::now();
        let finished = wait_until(Instant::now() + Duration::from_millis(300), || {
            capture.is_finished()
        });
        let (bytes, eof) = capture.take().unwrap();

        assert!(!finished && !eof);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(bytes, b"early");
    }

    #[test]
    fn abandoned_capture_stops_retaining_a_continuously_writing_escaped_child() {
        let tmp = tempfile::tempdir().unwrap();
        let pid_file = tmp.path().join("writer.pid");
        let script = format!(
            "printf early; (while echo x; do :; done) & echo $! > '{}'; exit 0",
            pid_file.display()
        );
        let mut child = Command::new("sh")
            .args(["-c", &script])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let capture = PipeCapture::spawn(child.stdout.take().unwrap());
        child.wait().unwrap();
        let writer = read_pid(&pid_file);
        let retained = Arc::downgrade(&capture.bytes);

        let (bytes, eof) = capture.take().unwrap();

        assert!(!eof);
        assert!(bytes.starts_with(b"early"));
        // The reader dropped its buffer and its read end: the writer dies on
        // EPIPE instead of growing memory nobody can reach.
        let released = wait_until(Instant::now() + Duration::from_secs(2), || {
            retained.upgrade().is_none()
        });
        let writer_gone = process_exits_within(writer, Duration::from_secs(2));
        unsafe {
            libc::kill(writer as libc::pid_t, libc::SIGKILL);
        }
        assert!(released && writer_gone);
    }

    #[test]
    fn dropped_capture_stops_retaining_a_continuously_writing_escaped_child() {
        let tmp = tempfile::tempdir().unwrap();
        let pid_file = tmp.path().join("writer.pid");
        let script = format!(
            "(while echo x; do :; done) & echo $! > '{}'; exit 0",
            pid_file.display()
        );
        let mut child = Command::new("sh")
            .args(["-c", &script])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let capture = PipeCapture::spawn(child.stdout.take().unwrap());
        child.wait().unwrap();
        let writer = read_pid(&pid_file);
        let retained = Arc::downgrade(&capture.bytes);

        // An early error return drops the capture without `take`.
        drop(capture);

        let released = wait_until(Instant::now() + Duration::from_secs(2), || {
            retained.upgrade().is_none()
        });
        let writer_gone = process_exits_within(writer, Duration::from_secs(2));
        unsafe {
            libc::kill(writer as libc::pid_t, libc::SIGKILL);
        }
        assert!(released && writer_gone);
    }

    /// Under an inherited `SIGCHLD=SIG_IGN` an exited child is reaped at
    /// once and `waitid` fails with ECHILD, so its pid and group id are no
    /// longer ours. Each caller must then report the failure without
    /// signalling. A member left in the group would die from any group
    /// signal, so it surviving proves none was sent. The disposition is
    /// process-wide, so this runs in its own re-executed test process.
    #[test]
    fn lost_child_is_never_signalled_under_ignored_sigchld() {
        const ISOLATED: &str = "GAH_TEST_SIGCHLD_IGNORED";
        if std::env::var_os(ISOLATED).is_none() {
            let name = concat!(
                module_path!(),
                "::lost_child_is_never_signalled_under_ignored_sigchld"
            );
            let name = name.split_once("::").unwrap().1;
            let output = Command::new(std::env::current_exe().unwrap())
                .args([name, "--exact", "--test-threads=1", "--nocapture"])
                .env(ISOLATED, "1")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{stdout}\n{stderr}"
            );
            return;
        }
        unsafe {
            libc::signal(libc::SIGCHLD, libc::SIG_IGN);
        }
        let tmp = tempfile::tempdir().unwrap();
        let script = |name: &str| {
            let pid_file = tmp.path().join(name);
            let script = format!("sleep 30 & echo $! > '{}'; exit 0", pid_file.display());
            (script, pid_file)
        };
        let survives = |pid_file: &std::path::Path| {
            let member = read_pid(pid_file);
            let survived = !process_exits_within(member, Duration::from_millis(500));
            unsafe {
                libc::kill(member as libc::pid_t, libc::SIGKILL);
            }
            survived
        };

        let (backend, backend_pid) = script("backend.pid");
        let mut command = Command::new("sh");
        command.args(["-c", &backend]);
        let log = tmp.path().join("backend.log");
        let (exit_code, _, _) =
            crate::runner::process::spawn_with_idle_watch(command, &log, tmp.path(), 60, "test")
                .unwrap();
        assert!(survives(&backend_pid), "backend group was signalled");
        assert_eq!(
            exit_code,
            crate::runner::process::PROCESS_CLEANUP_FAILED_EXIT_CODE
        );
        assert!(std::fs::read_to_string(&log)
            .unwrap()
            .contains("GAH: harness process cleanup failed: lost track"));

        let (validation, validation_pid) = script("validation.pid");
        let error = crate::validation_runner::validate(
            &[validation],
            tmp.path(),
            &[],
            Duration::from_secs(30),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("lost track"), "{error:#}");
        assert!(survives(&validation_pid), "validation group was signalled");

        let (git, git_pid) = script("git.pid");
        let mut command = Command::new("sh");
        command
            .args(["-c", &git])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::runner::prepare_process_group(&mut command);
        let child = command.spawn().unwrap();
        let error =
            crate::worktree::wait_with_timeout_for(child, "git test", Duration::from_secs(30))
                .unwrap_err();
        assert!(format!("{error:#}").contains("lost track"), "{error:#}");
        assert!(survives(&git_pid), "git group was signalled");
    }

    /// An inherited `SIGCHLD=SIG_IGN` (on Linux), or `SA_NOCLDWAIT`, makes
    /// the kernel reap children on exit. After the startup reset an exited
    /// child stays waitable until reaped, and backend cleanup signals its
    /// group again. The disposition is process-wide, so each case runs in its
    /// own re-executed test process; `SIG_IGN` arrives through exec as from a
    /// real launcher.
    #[test]
    fn startup_reset_keeps_children_waitable_under_auto_reaping_sigchld() {
        use std::os::unix::process::CommandExt;
        const ISOLATED: &str = "GAH_TEST_SIGCHLD_AUTO_REAP";
        let Ok(mode) = std::env::var(ISOLATED) else {
            let name = concat!(
                module_path!(),
                "::startup_reset_keeps_children_waitable_under_auto_reaping_sigchld"
            );
            let name = name.split_once("::").unwrap().1;
            for mode in ["inherited-ignore", "nocldwait"] {
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .args([name, "--exact", "--test-threads=1", "--nocapture"])
                    .env(ISOLATED, mode);
                if mode == "inherited-ignore" {
                    // Runs in the forked child only; this process keeps its
                    // own disposition.
                    unsafe {
                        command.pre_exec(|| {
                            libc::signal(libc::SIGCHLD, libc::SIG_IGN);
                            Ok(())
                        });
                    }
                }
                let output = command.output().unwrap();
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                assert!(
                    output.status.success() && stdout.contains("1 passed"),
                    "{mode}: {stdout}\n{stderr}"
                );
            }
            return;
        };
        if mode == "nocldwait" {
            unsafe {
                let mut action = std::mem::zeroed::<libc::sigaction>();
                action.sa_sigaction = libc::SIG_DFL;
                action.sa_flags = libc::SA_NOCLDWAIT;
                assert_eq!(
                    libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()),
                    0
                );
            }
        }
        let exits = || Command::new("sh").args(["-c", "exit 7"]).spawn().unwrap();
        let settle = |child: &Child| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let result = exited_without_reaping(child);
                if !matches!(result, Ok(false)) || Instant::now() > deadline {
                    return result;
                }
                thread::sleep(Duration::from_millis(10));
            }
        };

        let auto_reaping = || unsafe {
            let mut action = std::mem::zeroed::<libc::sigaction>();
            assert_eq!(
                libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action),
                0
            );
            action.sa_sigaction == libc::SIG_IGN || action.sa_flags & libc::SA_NOCLDWAIT != 0
        };
        assert!(
            auto_reaping(),
            "{mode}: no auto-reaping disposition in effect"
        );
        // Precondition: the kernel really reaps behind our back. macOS drops
        // the auto-reap of an exec-inherited SIG_IGN; Linux keeps it.
        if mode == "nocldwait" || cfg!(target_os = "linux") {
            let lost = settle(&exits()).unwrap_err();
            assert_eq!(lost.raw_os_error(), Some(libc::ECHILD), "{mode}");
        }

        keep_exited_children_waitable().unwrap();
        assert!(!auto_reaping(), "{mode}");

        let mut child = exits();
        assert!(settle(&child).unwrap(), "{mode}");
        thread::sleep(Duration::from_millis(200));
        assert!(exited_without_reaping(&child).unwrap(), "{mode}");
        assert_eq!(child.wait().unwrap().code(), Some(7), "{mode}");

        let tmp = tempfile::tempdir().unwrap();
        let pid_file = tmp.path().join("sleeper.pid");
        let mut command = Command::new("sh");
        command.args([
            "-c",
            &format!("sleep 30 & echo $! > '{}'; exit 3", pid_file.display()),
        ]);
        let log = tmp.path().join("backend.log");
        let (exit_code, _, _) =
            crate::runner::process::spawn_with_idle_watch(command, &log, tmp.path(), 60, "test")
                .unwrap();
        assert_eq!(exit_code, 3, "{mode}");
        assert!(
            process_exits_within(read_pid(&pid_file), Duration::from_secs(2)),
            "{mode}: backend group was not signalled"
        );
    }

    #[test]
    fn group_is_signalled_while_its_exited_leader_still_reserves_the_id() {
        let tmp = tempfile::tempdir().unwrap();
        let pid_file = tmp.path().join("sleeper.pid");
        let mut command = Command::new("sh");
        command.args([
            "-c",
            &format!("sleep 30 & echo $! > '{}'; exit 3", pid_file.display()),
        ]);
        crate::runner::prepare_process_group(&mut command);
        let mut child = command.spawn().unwrap();
        assert!(wait_until(Instant::now() + Duration::from_secs(5), || {
            exited_without_reaping(&child).unwrap()
        }));

        // Exited but unreaped: the pid (and group id) is still taken.
        assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, 0) }, 0);
        kill_unreaped_process_group(&child);
        assert_eq!(child.wait().unwrap().code(), Some(3));
        assert!(process_exits_within(
            read_pid(&pid_file),
            Duration::from_secs(2)
        ));
    }
}
