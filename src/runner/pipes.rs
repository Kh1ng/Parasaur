//! Bounded collection of a child's output pipes.
//!
//! A child's stdout/stderr reach EOF only once every process holding the
//! write ends has closed them. Descendants inherit those descriptors, so a
//! backgrounded helper keeps them open after the direct child has exited.
//! Callers therefore terminate leftover members of the child's process group
//! and wait for EOF for at most a short grace period; output read before the
//! deadline is kept either way.

use std::io::Read;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// How long a caller waits for output pipes to reach EOF after the process
/// group that owned them has exited or been killed.
pub(crate) const PIPE_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// SIGKILL every process still in process group `pgid`.
///
/// Unlike `kill_process_group`, this needs no live child handle, so it is
/// valid after the group leader has been reaped: a process group id stays
/// reserved while any member remains, so the signal reaches only leftover
/// members of that group. An empty group makes this a no-op (ESRCH).
pub(crate) fn kill_process_group_id(pgid: u32) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(-(pgid as libc::pid_t), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pgid;
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
/// stop waiting at a deadline and still keep the bytes read so far.
pub(crate) struct PipeCapture {
    bytes: Arc<Mutex<Vec<u8>>>,
    reader: JoinHandle<std::io::Result<()>>,
}

impl PipeCapture {
    pub(crate) fn spawn<R: Read + Send + 'static>(mut pipe: R) -> Self {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&bytes);
        let reader = thread::spawn(move || {
            let mut buf = [0_u8; 8192];
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => return Ok(()),
                    Ok(read) => sink
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .extend_from_slice(&buf[..read]),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
        });
        Self { bytes, reader }
    }

    /// True once the pipe reached EOF (or the read failed).
    pub(crate) fn is_finished(&self) -> bool {
        self.reader.is_finished()
    }

    /// Return the bytes read so far and whether the pipe reached EOF. A
    /// still-open pipe is left to its reader thread, which exits on its own
    /// once the last holder closes it; nothing waits on it.
    pub(crate) fn take(self) -> std::io::Result<(Vec<u8>, bool)> {
        if !self.reader.is_finished() {
            let bytes = self
                .bytes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            return Ok((bytes, false));
        }
        self.reader
            .join()
            .map_err(|_| std::io::Error::other("pipe reader panicked"))??;
        let bytes = std::mem::take(&mut *self.bytes.lock().unwrap_or_else(PoisonError::into_inner));
        Ok((bytes, true))
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
}
