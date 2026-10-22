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
pub(crate) fn exited_without_reaping(child: &Child) -> std::io::Result<bool> {
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
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { info.assume_init().si_pid() } != 0)
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
    /// `None` once `take` abandoned a still-open pipe.
    bytes: Arc<Mutex<Option<Vec<u8>>>>,
    reader: JoinHandle<std::io::Result<()>>,
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
        Self { bytes, reader }
    }

    /// True once the pipe reached EOF (or the read failed).
    pub(crate) fn is_finished(&self) -> bool {
        self.reader.is_finished()
    }

    /// Return the bytes read so far and whether the pipe reached EOF. A
    /// still-open pipe is abandoned: its reader keeps nothing more and exits
    /// at its next read, or once the last holder closes the pipe.
    // ponytail: a silent holder parks the reader (one thread + fd, no growth)
    // until it writes or exits; poll(2) with a cancel flag if that matters.
    pub(crate) fn take(self) -> std::io::Result<(Vec<u8>, bool)> {
        let finished = self.reader.is_finished();
        if finished {
            self.reader
                .join()
                .map_err(|_| std::io::Error::other("pipe reader panicked"))??;
        }
        let bytes = self
            .bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .unwrap_or_default();
        Ok((bytes, finished))
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
