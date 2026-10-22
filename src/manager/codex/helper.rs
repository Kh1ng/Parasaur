use super::make_nonblocking;
use crate::runner::pipes::exited_without_reaping;
use anyhow::{anyhow, Context};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::process::{Child, Command, Output, Stdio};
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

const CAPTURE_MAX_BYTES: usize = 64 * 1024;
const CLEANUP_GRACE: Duration = Duration::from_millis(250);
#[cfg(test)]
pub(super) static FAIL_HELPER_CLEANUP_AFTER_REAP: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
pub(super) static FAIL_HELPER_NONBLOCKING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
thread_local! {
    /// Make this thread's next helper exit check report ECHILD and record
    /// the pid it lost.
    static LOSE_NEXT_HELPER_CHECK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static LOST_HELPER_PID: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static HELPER_CHILD_OPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    eof: bool,
    read_failed: bool,
    overflowed: bool,
}

impl Capture {
    fn drain(&mut self, reader: &mut impl Read) -> std::io::Result<()> {
        if self.eof || self.read_failed {
            return Ok(());
        }
        let mut chunk = [0_u8; 8192];
        match reader.read(&mut chunk) {
            Ok(0) => self.eof = true,
            Ok(read) => {
                let retained = CAPTURE_MAX_BYTES.saturating_sub(self.bytes.len());
                self.bytes.extend_from_slice(&chunk[..read.min(retained)]);
                self.overflowed |= read > retained;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => {
                self.read_failed = true;
                return Err(error);
            }
        }
        Ok(())
    }

    fn finished(&self) -> bool {
        self.eof || self.read_failed
    }
}

fn drain_reader(capture: &mut Capture, reader: &mut Option<impl Read>) -> std::io::Result<()> {
    match reader {
        Some(reader) => capture.drain(reader),
        None => Ok(()),
    }
}

fn cleanup_overflow(stdout: &Capture, stderr: &Capture, context: &str) -> Option<String> {
    let stream = if stdout.overflowed {
        "stdout"
    } else if stderr.overflowed {
        "stderr"
    } else {
        return None;
    };
    Some(format!(
        "{context} {stream} exceeded {CAPTURE_MAX_BYTES} bytes during cleanup"
    ))
}

fn make_helper_pipe_nonblocking(fd: std::os::fd::RawFd) -> std::io::Result<()> {
    #[cfg(test)]
    if FAIL_HELPER_NONBLOCKING.swap(false, Ordering::SeqCst) {
        return Err(std::io::Error::other("injected helper nonblocking failure"));
    }
    make_nonblocking(fd)
}

pub(super) enum HelperCommandFailure {
    Command,
    Fatal(anyhow::Error),
}

impl From<anyhow::Error> for HelperCommandFailure {
    fn from(_: anyhow::Error) -> Self {
        Self::Command
    }
}

/// Counts every check, signal and reap of the helper child in tests.
fn count_child_op() {
    #[cfg(test)]
    HELPER_CHILD_OPS.with(|ops| ops.set(ops.get() + 1));
}

fn helper_exited(child: &Child) -> std::io::Result<bool> {
    count_child_op();
    #[cfg(test)]
    if LOSE_NEXT_HELPER_CHECK.with(|lose| lose.replace(false)) {
        LOST_HELPER_PID.with(|pid| pid.set(child.id()));
        return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
    }
    exited_without_reaping(child)
}

fn terminate_group(child: &mut std::process::Child, exited: bool) -> Option<String> {
    count_child_op();
    let result = unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    if result == -1 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH)
            && !(exited && error.raw_os_error() == Some(libc::EPERM))
        {
            let _ = child.kill();
            return Some(format!("signaling helper process group: {error}"));
        }
    }
    let _ = child.kill();
    None
}

pub(super) fn bounded_command_output(
    command: &mut Command,
    context: &str,
    timeout: Duration,
) -> Result<Output, HelperCommandFailure> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::runner::process::prepare_process_group(command);
    let mut child = command
        .spawn()
        .with_context(|| format!("starting {context}"))?;
    let deadline = Instant::now() + timeout;
    let cleanup_deadline = deadline + CLEANUP_GRACE;
    let mut stdout_reader = Some(child.stdout.take().expect("piped helper stdout"));
    let mut stderr_reader = Some(child.stderr.take().expect("piped helper stderr"));
    let mut stdout = Capture::default();
    let mut stderr = Capture::default();
    let mut status = None;
    let mut failure: Option<(bool, anyhow::Error)> = None;
    let mut setup_failures = Vec::new();
    // Set once a check fails: the pid, and so the group id, may now name
    // other processes. Terminal: never query, signal or reap the child again.
    let mut lost = false;
    if let Some(reader) = stdout_reader.as_ref() {
        if let Err(error) = make_helper_pipe_nonblocking(reader.as_raw_fd()) {
            setup_failures.push(format!("making {context} stdout nonblocking: {error}"));
            stdout.read_failed = true;
            stdout_reader = None;
        }
    }
    if let Some(reader) = stderr_reader.as_ref() {
        if let Err(error) = make_helper_pipe_nonblocking(reader.as_raw_fd()) {
            setup_failures.push(format!("making {context} stderr nonblocking: {error}"));
            stderr.read_failed = true;
            stderr_reader = None;
        }
    }
    if !setup_failures.is_empty() {
        failure = Some((true, anyhow!(setup_failures.join("; "))));
    }

    while failure.is_none() {
        if let Err(error) = drain_reader(&mut stdout, &mut stdout_reader) {
            failure = Some((true, anyhow!("reading {context} stdout: {error}")));
            break;
        }
        if let Err(error) = drain_reader(&mut stderr, &mut stderr_reader) {
            failure = Some((true, anyhow!("reading {context} stderr: {error}")));
            break;
        }
        if stdout.overflowed || stderr.overflowed {
            let stream = if stdout.overflowed {
                "stdout"
            } else {
                "stderr"
            };
            failure = Some((
                true,
                anyhow!("{context} {stream} exceeded {CAPTURE_MAX_BYTES} bytes"),
            ));
            break;
        }
        match helper_exited(&child) {
            Ok(true) => {
                break;
            }
            Ok(false) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(false) => {
                failure = Some((false, anyhow!("{context} timed out after {timeout:?}")));
                break;
            }
            Err(error) => {
                lost = true;
                failure = Some((true, anyhow!("lost track of {context}: {error}")));
                break;
            }
        }
    }

    let mut cleanup_failures = Vec::new();
    let mut cleanup_deadline = cleanup_deadline;
    if !lost {
        match helper_exited(&child) {
            Ok(exited) => {
                if let Some(error) = terminate_group(&mut child, exited) {
                    cleanup_failures.push(error);
                }
            }
            Err(error) => {
                lost = true;
                cleanup_failures.push(format!("lost track of {context} before cleanup: {error}"));
            }
        }
    }
    if lost {
        // Its exit can never be confirmed, so give the pipes only the grace
        // period rather than the rest of the command timeout.
        cleanup_deadline = cleanup_deadline.min(Instant::now() + CLEANUP_GRACE);
    }
    let mut overflow_reported = stdout.overflowed || stderr.overflowed;
    if let Err(error) = drain_reader(&mut stdout, &mut stdout_reader) {
        cleanup_failures.push(format!("reading stdout during cleanup: {error}"));
    }
    if let Err(error) = drain_reader(&mut stderr, &mut stderr_reader) {
        cleanup_failures.push(format!("reading stderr during cleanup: {error}"));
    }
    if !overflow_reported {
        if let Some(error) = cleanup_overflow(&stdout, &stderr, context) {
            cleanup_failures.push(error);
            overflow_reported = true;
        }
    }
    // ponytail: A configured Codex executable is trusted. If untrusted helper
    // containment becomes a requirement, use a per-invocation cgroup or a
    // dedicated supervisor instead of process-wide subreaping or PID scans.
    loop {
        if let Err(error) = drain_reader(&mut stdout, &mut stdout_reader) {
            cleanup_failures.push(format!("reading stdout during cleanup: {error}"));
        }
        if let Err(error) = drain_reader(&mut stderr, &mut stderr_reader) {
            cleanup_failures.push(format!("reading stderr during cleanup: {error}"));
        }
        if !overflow_reported {
            if let Some(error) = cleanup_overflow(&stdout, &stderr, context) {
                cleanup_failures.push(error);
                overflow_reported = true;
            }
        }
        if status.is_none() && !lost {
            count_child_op();
            match child.try_wait() {
                Ok(Some(exit)) => status = Some(exit),
                Ok(None) => {}
                Err(error) => {
                    cleanup_failures.push(format!("reaping helper: {error}"));
                    break;
                }
            }
        }
        if (status.is_some() || lost) && stdout.finished() && stderr.finished() {
            break;
        }
        if Instant::now() >= cleanup_deadline {
            if status.is_none() && !lost {
                cleanup_failures.push("helper was not reaped before cleanup deadline".into());
            }
            if !stdout.finished() || !stderr.finished() {
                cleanup_failures.push("capture pipe remained open after helper cleanup".into());
            }
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    #[cfg(test)]
    if FAIL_HELPER_CLEANUP_AFTER_REAP.swap(false, Ordering::SeqCst) {
        cleanup_failures.push("injected unconfirmed helper cleanup".into());
    }
    if !cleanup_failures.is_empty() {
        let cleanup = cleanup_failures.join("; ");
        let error = match failure {
            Some((_, failure)) => anyhow!("{failure:#}; helper cleanup failed: {cleanup}"),
            None => anyhow!("{context} exited but helper cleanup failed: {cleanup}"),
        };
        return Err(HelperCommandFailure::Fatal(error));
    }
    if let Some((fatal, failure)) = failure {
        return Err(if fatal {
            HelperCommandFailure::Fatal(failure)
        } else {
            let _ = failure;
            HelperCommandFailure::Command
        });
    }
    Ok(Output {
        status: status.expect("successful helper was reaped"),
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waitid_observes_exit_without_reaping_the_group_leader() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 0"]);
        crate::runner::process::prepare_process_group(&mut command);
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !exited_without_reaping(&child).unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        assert!(terminate_group(&mut child, true).is_none());
        assert!(child.try_wait().unwrap().is_some());
    }

    /// After a failed exit check the pid may name another process, so the
    /// helper never checks, signals or reaps it again. The injected ECHILD
    /// leaves the real child running: any later check, group kill or reap
    /// would show.
    #[test]
    fn lost_helper_is_never_checked_signalled_or_reaped_again() {
        let mut command = Command::new("sleep");
        command.arg("30");
        HELPER_CHILD_OPS.with(|ops| ops.set(0));
        LOSE_NEXT_HELPER_CHECK.with(|lose| lose.set(true));
        let started = Instant::now();

        let result = bounded_command_output(&mut command, "lost helper", Duration::from_secs(30));

        let elapsed = started.elapsed();
        let pid = LOST_HELPER_PID.with(std::cell::Cell::get) as libc::pid_t;
        assert_ne!(pid, 0, "no helper check was lost");
        let untouched = unsafe { libc::waitpid(pid, std::ptr::null_mut(), libc::WNOHANG) } == 0;
        unsafe {
            libc::kill(pid, libc::SIGKILL);
            libc::waitpid(pid, std::ptr::null_mut(), 0);
        }
        let error = match result {
            Err(HelperCommandFailure::Fatal(error)) => format!("{error:#}"),
            Err(HelperCommandFailure::Command) => "command failure".into(),
            Ok(_) => "success".into(),
        };
        assert!(untouched, "lost helper was signalled or reaped");
        assert_eq!(HELPER_CHILD_OPS.with(std::cell::Cell::get), 1);
        assert!(error.contains("lost track of lost helper"), "{error}");
        assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    }

    #[test]
    fn final_cleanup_drain_rechecks_the_capture_limit() {
        let mut capture = Capture {
            bytes: vec![0; CAPTURE_MAX_BYTES],
            ..Capture::default()
        };
        capture.drain(&mut std::io::Cursor::new([1])).unwrap();

        assert_eq!(
            cleanup_overflow(&capture, &Capture::default(), "helper").as_deref(),
            Some("helper stdout exceeded 65536 bytes during cleanup")
        );
    }
}
