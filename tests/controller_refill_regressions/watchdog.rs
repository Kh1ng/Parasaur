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
            let processes = process_tree(pid);
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            Some(processes)
        } else {
            None
        }
    });
    let output = child.wait_with_output().unwrap();
    let _ = finished.send(());
    if let Some(processes) = watchdog.join().unwrap() {
        panic!(
            "controller did not finish within {timeout:?}\nprocesses:\n{processes}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    output
}

/// `ps` lines for `root` and every descendant, taken before a stuck
/// controller is killed: which worker is still running, its state and
/// kernel wait channel, and its arguments (#1662). Backends run in their
/// own process groups, so this follows parent PIDs rather than the group.
pub(super) fn process_tree(root: i32) -> String {
    let listing = match std::process::Command::new("ps")
        .args(["-eo", "pid=,ppid=,stat=,wchan:24=,etimes=,args="])
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
        Err(error) => return format!("ps failed: {error}"),
    };
    let rows: Vec<(i32, i32, &str)> = listing
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            Some((pid, ppid, line))
        })
        .collect();
    let mut tree = vec![root];
    let mut index = 0;
    while index < tree.len() {
        let parent = tree[index];
        tree.extend(
            rows.iter()
                .filter(|(_, ppid, _)| *ppid == parent)
                .map(|(pid, _, _)| *pid),
        );
        index += 1;
    }
    rows.iter()
        .filter(|(pid, _, _)| tree.contains(pid))
        .map(|(_, _, line)| *line)
        .collect::<Vec<_>>()
        .join("\n")
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
