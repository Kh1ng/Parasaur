//! Copies a child's output stream into a redacted log file on its own thread.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

/// Per stream, bound both buffered input and redacted output. A backend can
/// produce an endless line; never split it and risk exposing a partial secret.
pub(crate) const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
const LIMIT_NOTICE: &str =
    "\nGAH: output capture exceeded the 16 MiB per-stream limit; remaining output omitted.\n";

/// Append `reader` to `path` one redacted line at a time, sending a progress
/// tick after each read. Dropping the receiver abandons the copy: the thread
/// discards its partial line and closes `reader` at its next read.
/// Returns true only when EOF was captured completely. After the size limit,
/// continue draining without retaining bytes so the child cannot block on its pipe.
pub(crate) fn copy_stream_to_file<R: Read + Send + 'static>(
    mut reader: R,
    path: PathBuf,
    progress_tx: Option<mpsc::SyncSender<()>>,
) -> thread::JoinHandle<bool> {
    thread::spawn(move || {
        let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) else {
            return false;
        };
        let mut buf = [0_u8; 8192];
        let mut pending = Vec::new();
        let mut input_bytes = 0usize;
        let mut written = 0usize;
        let mut complete = true;
        loop {
            let read = match reader.read(&mut buf) {
                Ok(read) => read,
                Err(_) => return false,
            };
            if read == 0 {
                break;
            }
            if complete {
                input_bytes += read;
                if input_bytes > MAX_CAPTURE_BYTES {
                    complete = false;
                } else {
                    // Scan only new bytes, not the entire unfinished line on
                    // every read (which made long lines quadratic to process).
                    for part in buf[..read].split_inclusive(|byte| *byte == b'\n') {
                        pending.extend_from_slice(part);
                        if part.ends_with(b"\n") {
                            let text = crate::redact::redact(&String::from_utf8_lossy(&pending));
                            if text.len() > MAX_CAPTURE_BYTES - written {
                                complete = false;
                                break;
                            }
                            if file.write_all(text.as_bytes()).is_err() {
                                return false;
                            }
                            written += text.len();
                            pending.clear();
                        }
                    }
                }
                if !complete {
                    pending.clear();
                    if file
                        .write_all(crate::redact::redact(LIMIT_NOTICE).as_bytes())
                        .is_err()
                    {
                        return false;
                    }
                }
            }
            let _ = file.flush();
            // The caller dropped its receiver: it stopped collecting. Keep
            // nothing more and close the read end, so an escaped writer gets
            // EPIPE instead of growing `pending` nobody will read.
            if progress_tx.as_ref().is_some_and(|tx| {
                matches!(tx.try_send(()), Err(mpsc::TrySendError::Disconnected(())))
            }) {
                return false;
            }
        }
        if complete && !pending.is_empty() {
            let text = crate::redact::redact(&String::from_utf8_lossy(&pending));
            if text.len() > MAX_CAPTURE_BYTES - written {
                complete = false;
                if file
                    .write_all(crate::redact::redact(LIMIT_NOTICE).as_bytes())
                    .is_err()
                {
                    return false;
                }
            } else if file.write_all(text.as_bytes()).is_err() {
                return false;
            }
        }
        file.flush().is_ok() && complete
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::pipes::wait_until;
    use std::time::{Duration, Instant};

    const SECRET: &str = "sk-abcdefghijklmnopqrstuvwxyz0123";

    #[test]
    fn copies_redacted_lines_and_the_final_partial_line() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.log");
        let input = format!("héllo {SECRET}\ntail {SECRET}");
        let (tx, rx) = mpsc::sync_channel(1);

        copy_stream_to_file(std::io::Cursor::new(input), path.clone(), Some(tx))
            .join()
            .unwrap();

        let log = fs::read_to_string(path).unwrap();
        assert!(log.starts_with("héllo ") && log.contains("\ntail "));
        assert!(!log.contains(SECRET));
        assert!(rx.try_iter().count() > 0);
    }

    #[test]
    fn oversized_unfinished_line_is_drained_without_emitting_a_partial_secret() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.log");
        let input = std::io::repeat(b'x')
            .take((MAX_CAPTURE_BYTES - SECRET.len() / 2) as u64)
            .chain(std::io::Cursor::new(format!("{SECRET}\nignored tail\n")));
        let (tx, rx) = mpsc::sync_channel(1);
        let copier = copy_stream_to_file(input, path.clone(), Some(tx));
        assert!(wait_until(Instant::now() + Duration::from_secs(5), || {
            copier.is_finished()
        }));
        assert!(!copier.join().unwrap());
        let log = fs::read_to_string(path).unwrap();
        assert!(log.contains("output capture exceeded"));
        assert!(log.len() < 256);
        assert!(!log.contains("sk-") && !log.contains("xxxx"));
        assert_eq!(
            rx.try_iter().count(),
            1,
            "progress is coalesced even when not consumed"
        );
    }

    #[test]
    fn an_undrained_progress_receiver_does_not_block_or_accumulate_ticks() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.log");
        let mut bytes = vec![b'x'; 2 * 1024 * 1024];
        for line in bytes.chunks_mut(8192) {
            *line.last_mut().unwrap() = b'\n';
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let copier =
            copy_stream_to_file(std::io::Cursor::new(bytes.clone()), path.clone(), Some(tx));
        assert!(wait_until(Instant::now() + Duration::from_secs(10), || {
            copier.is_finished()
        }));
        assert!(copier.join().unwrap());
        assert_eq!(fs::read(path).unwrap(), bytes);
        assert_eq!(rx.try_iter().count(), 1);
    }

    #[test]
    fn dropped_receiver_stops_a_newline_free_endless_stream() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("out.log");
        let (tx, rx) = mpsc::sync_channel(1);
        let copier = copy_stream_to_file(std::io::repeat(b'x'), path.clone(), Some(tx));

        drop(rx);

        assert!(wait_until(Instant::now() + Duration::from_secs(2), || {
            copier.is_finished()
        }));
        // The abandoned partial line is discarded, never flushed.
        assert_eq!(fs::read_to_string(path).unwrap(), "");
    }

    /// A backend leaves behind a writer in its own session that never stops
    /// and never writes a newline. Group cleanup cannot reach it, so the run
    /// returns after the drain grace; the copier must then close the pipe
    /// rather than buffer the writer's output forever. The writer exits on
    /// that EPIPE and records it, so nothing here depends on its pid.
    #[cfg(unix)]
    #[test]
    fn backend_return_releases_the_copier_of_an_escaped_endless_writer() {
        use crate::runner::backends::test_util::{fixture, make_fake_bin};
        use std::process::Command;

        let _exec_guard = crate::test_support::ExecGuard::new();
        let f = fixture();
        let stop = f.session_dir.join("stop");
        let closed = f.session_dir.join("closed");
        // The subshell dies on SIGPIPE (or stops on a failed write); the
        // session leader then records the closed pipe. `stop` ends a writer
        // the copier failed to release.
        let writer = format!(
            "(while [ ! -e \"{}\" ] && printf x; do :; done); touch \"{}\"",
            stop.display(),
            closed.display()
        );
        let body = format!(
            "#!/bin/sh\necho 'héllo {SECRET}'\nwriter='{writer}'\n\
             if command -v setsid >/dev/null 2>&1; then setsid sh -c \"$writer\" 2>/dev/null &\n\
             else perl -MPOSIX -e 'POSIX::setsid(); exec @ARGV' sh -c \"$writer\" 2>/dev/null &\n\
             fi\nsleep 0.2\n"
        );
        make_fake_bin(&f.bin_dir, "backend", &body);
        let log_path = f.session_dir.join("backend-output.log");
        let started = Instant::now();

        let result = crate::runner::process::spawn_with_idle_watch(
            Command::new(f.bin_dir.join("backend")),
            &log_path,
            &f.worktree,
            60,
            "launching escaped-writer test backend",
        )
        .unwrap();
        let elapsed = started.elapsed();
        let released = wait_until(Instant::now() + Duration::from_secs(2), || closed.exists());
        fs::write(&stop, "").unwrap();

        assert_eq!(result.0, 0);
        assert!(
            elapsed < Duration::from_secs(10),
            "returned after {elapsed:?}"
        );
        assert!(
            released,
            "copier kept reading the escaped writer after return"
        );
        let log = fs::read_to_string(&log_path).unwrap();
        assert!(log.contains("héllo ") && !log.contains(SECRET));
        assert!(log.contains("still held open by a process outside its process group"));
        assert!(!log.contains("xxxx"), "abandoned partial line was flushed");
    }
}
