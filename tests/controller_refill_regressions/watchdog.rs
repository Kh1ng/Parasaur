use super::ProcessGroupGuard;
use std::process::Output;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// Drain controller output immediately, but fail with that output if the
/// fixture's synchronization never completes.
pub(super) fn wait_with_output(child: ProcessGroupGuard) -> Output {
    wait_with_timeout(child, Duration::from_secs(120))
}

fn wait_with_timeout(child: ProcessGroupGuard, timeout: Duration) -> Output {
    let pid = child.id() as i32;
    let (finished, receive) = mpsc::channel();
    let watchdog = std::thread::spawn(move || {
        if matches!(
            receive.recv_timeout(timeout),
            Err(RecvTimeoutError::Timeout)
        ) {
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            true
        } else {
            false
        }
    });
    let output = child.wait_with_output().unwrap();
    let _ = finished.send(());
    assert!(
        !watchdog.join().unwrap(),
        "controller did not finish within {timeout:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

#[test]
#[should_panic(expected = "controller did not finish within")]
fn a_stuck_controller_fails_instead_of_hanging() {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let child = Command::new("/bin/sh")
        .args(["-c", "echo controller-stuck; exec sleep 30"])
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_with_timeout(ProcessGroupGuard::new(child), Duration::from_millis(10));
}
