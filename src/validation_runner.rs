use crate::runner::PipeCapture;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const VALIDATION_COMMAND_TIMEOUT_EXIT_CODE: i32 = 124;

fn is_validation_timeout_error(error: &str) -> bool {
    error.contains("timed out after")
}

/// Run validation commands sequentially in the worktree and surface the first
/// command failure with its combined output.
pub(crate) fn validate(
    commands: &[String],
    worktree: &Path,
    env_vars: &[(String, String)],
    timeout: Duration,
) -> Result<()> {
    for command in commands {
        if command.trim().is_empty() {
            continue;
        }
        println!("  Validating: {command}");
        let output = run_shell_command(command, worktree, env_vars, timeout)
            .with_context(|| format!("failed to run '{command}'"))?;
        if !output.status.success() {
            bail!(
                "$ {}\n{}{}",
                command,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }
    Ok(())
}

/// Run validation while preserving the first failing command's exit code for
/// baseline failure classification.
pub(crate) fn validate_with_exit_code(
    commands: &[String],
    worktree: &Path,
    env_vars: &[(String, String)],
    timeout: Duration,
) -> Result<(), (String, Option<i32>)> {
    for command in commands {
        if command.trim().is_empty() {
            continue;
        }
        println!("  Validating: {command}");
        let output = run_shell_command(command, worktree, env_vars, timeout).map_err(|error| {
            let error_text = format!("failed to run '{command}': {error:#}");
            let exit_code = is_validation_timeout_error(&error_text)
                .then_some(VALIDATION_COMMAND_TIMEOUT_EXIT_CODE);
            (error_text, exit_code)
        })?;
        if !output.status.success() {
            return Err((
                format!(
                    "$ {}\n{}{}",
                    command,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                ),
                output.status.code(),
            ));
        }
    }
    Ok(())
}

fn run_shell_command(
    command: &str,
    worktree: &Path,
    env_vars: &[(String, String)],
    timeout: Duration,
) -> Result<Output> {
    run_shell_command_with_shutdown(command, worktree, env_vars, timeout, || {
        crate::runner::shutdown_requested()
    })
}

fn run_shell_command_with_shutdown(
    command_text: &str,
    worktree: &Path,
    env_vars: &[(String, String)],
    timeout: Duration,
    shutdown_requested: impl Fn() -> bool,
) -> Result<Output> {
    if shutdown_requested() {
        bail!("shutdown requested before validation command '{command_text}'");
    }

    let mut command = Command::new("sh");
    command
        .args(["-c", command_text])
        .current_dir(worktree)
        // Validation is always unattended. Inheriting a manager/dispatch PTY
        // makes this newly-created process group a background terminal group;
        // a nested PTY helper such as `script(1)` can then receive SIGTTOU and
        // stop the entire validation tree indefinitely.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("GAH_TEST_NOW");
    for (key, value) in env_vars {
        command.env(key, value);
    }
    crate::runner::prepare_process_group(&mut command);

    let mut child = command
        .spawn()
        .with_context(|| format!("spawning validation command '{command_text}'"))?;
    let stdout = PipeCapture::spawn(child.stdout.take().context("capturing validation stdout")?);
    let stderr = PipeCapture::spawn(child.stderr.take().context("capturing validation stderr")?);

    let started = Instant::now();
    let waiting = || format!("waiting for validation command '{command_text}'");
    loop {
        match crate::runner::exited_without_reaping(&child) {
            Ok(true) => break,
            Ok(false) => {}
            // Not provably ours any more (ECHILD: already reaped), so neither
            // its pid nor its group id may be signalled. Dropping the
            // captures stops them retaining output.
            Err(error) => bail!(
                "validation process cleanup failed: lost track of validation command '{command_text}': {error}"
            ),
        }
        if started.elapsed() >= timeout {
            let cleanup_error = crate::runner::kill_process_group(&mut child);
            let _ = child.wait();
            collect_output(stdout, stderr)?;
            if let Some(error) = cleanup_error {
                bail!("validation process cleanup failed: {error}");
            }
            bail!(
                "validation command '{command_text}' timed out after {:.1}s (configured timeout {:.1}s)",
                started.elapsed().as_secs_f64(),
                timeout.as_secs_f64(),
            );
        }
        if shutdown_requested() {
            let cleanup_error = crate::runner::kill_process_group(&mut child);
            let _ = child.wait();
            collect_output(stdout, stderr)?;
            if let Some(error) = cleanup_error {
                bail!("validation process cleanup failed during shutdown: {error}");
            }
            bail!("shutdown requested during validation command '{command_text}'");
        }
        thread::sleep(Duration::from_millis(50));
    }

    // Validation is a bounded check, so anything the command left running in
    // its process group (a backgrounded server, a helper a test leaked) ends
    // with it. A survivor would otherwise outlive the dispatch and keep the
    // output pipes open, so the readers below would never see EOF. Signal
    // before reaping so the group id cannot have been reused.
    crate::runner::kill_unreaped_process_group(&child);
    let status = child.wait().with_context(waiting)?;
    let (stdout, stderr) = collect_output(stdout, stderr)?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Collect both streams once the command's process group is gone, waiting at
/// most `PIPE_DRAIN_GRACE` for EOF. A pipe still held by a process that left
/// the group yields the output read so far plus a note, never a hang.
fn collect_output(stdout: PipeCapture, stderr: PipeCapture) -> Result<(Vec<u8>, Vec<u8>)> {
    crate::runner::wait_until(Instant::now() + crate::runner::PIPE_DRAIN_GRACE, || {
        stdout.is_finished() && stderr.is_finished()
    });
    let (stdout, stdout_closed) = stdout.take().context("reading validation stdout")?;
    let (mut stderr, stderr_closed) = stderr.take().context("reading validation stderr")?;
    if !(stdout_closed && stderr_closed) {
        stderr.extend_from_slice(
            b"\nGAH: validation output was still held open by a process outside the command's process group; output may be incomplete.\n",
        );
    }
    Ok((stdout, stderr))
}

#[cfg(test)]
mod tests {
    use super::{run_shell_command_with_shutdown, validate};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    #[test]
    fn validation_clock_override_requires_explicit_opt_in() {
        const CHILD: &str = "GAH_VALIDATION_CLOCK_PROBE";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "validation_runner::tests::validation_clock_override_requires_explicit_opt_in",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env("GAH_TEST_NOW", "2026-01-01T00:00:00Z")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        for explicit in [false, true] {
            let env = if explicit {
                vec![("GAH_TEST_NOW".into(), "2030-01-01T00:00:00Z".into())]
            } else {
                vec![]
            };
            let output = run_shell_command_with_shutdown(
                r#"printf '%s' "${GAH_TEST_NOW-unset}""#,
                tmp.path(),
                &env,
                Duration::from_secs(5),
                || false,
            )
            .unwrap();
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                if explicit {
                    "2030-01-01T00:00:00Z"
                } else {
                    "unset"
                }
            );
        }
    }

    #[test]
    fn supports_shell_syntax() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        validate(
            &["cd sub && true".into()],
            tmp.path(),
            &[],
            std::time::Duration::from_secs(30),
        )
        .unwrap();
    }

    #[test]
    fn reports_failing_command_output() {
        let tmp = tempfile::tempdir().unwrap();
        let error = validate(
            &["echo oops >&2 && false".into()],
            tmp.path(),
            &[],
            std::time::Duration::from_secs(30),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("oops"));
    }

    #[test]
    fn propagates_validation_environment() {
        let tmp = tempfile::tempdir().unwrap();
        let observed = tmp.path().join("observed-target");
        let expected = tmp.path().join("shared-cargo-target");
        let commands = vec![format!(
            "printf '%s' \"$CARGO_TARGET_DIR\" > {}",
            observed.display()
        )];
        let environment = vec![(
            "CARGO_TARGET_DIR".to_string(),
            expected.to_string_lossy().into_owned(),
        )];

        validate(
            &commands,
            tmp.path(),
            &environment,
            std::time::Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(observed).unwrap(),
            expected.display().to_string()
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn validation_commands_receive_noninteractive_stdin() {
        let tmp = tempfile::tempdir().unwrap();
        let output = run_shell_command_with_shutdown(
            "readlink /proc/self/fd/0",
            tmp.path(),
            &[],
            std::time::Duration::from_secs(30),
            || false,
        )
        .unwrap();

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "/dev/null");
    }

    #[test]
    fn shutdown_kills_the_command_process_group_promptly() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("orphan-marker");
        let command = format!("(sleep 2; printf orphaned > '{}') & wait", marker.display());
        let shutdown = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&shutdown);
        let trigger_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            trigger.store(true, Ordering::SeqCst);
        });

        let started = Instant::now();
        let error = run_shell_command_with_shutdown(
            &command,
            tmp.path(),
            &[],
            std::time::Duration::from_secs(1),
            || shutdown.load(Ordering::SeqCst),
        )
        .unwrap_err();
        trigger_thread.join().unwrap();

        assert!(error.to_string().contains("shutdown requested"));
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "validation shutdown should not wait for the shell command"
        );
        std::thread::sleep(Duration::from_millis(2100));
        assert!(
            !marker.exists(),
            "the validation command's background child must die with its process group"
        );
    }

    #[test]
    #[cfg(unix)]
    fn normal_exit_reaps_background_children_holding_the_output_pipes() {
        let tmp = tempfile::tempdir().unwrap();
        let pid_file = tmp.path().join("sleeper.pid");
        let command = format!("sleep 30 & echo $! > '{}'; true", pid_file.display());

        let started = Instant::now();
        validate(&[command], tmp.path(), &[], Duration::from_secs(5)).unwrap();

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "validation must not wait for a background child to close its pipes"
        );
        let pid = crate::runner::pipes::read_pid(&pid_file);
        assert!(
            crate::runner::pipes::process_exits_within(pid, Duration::from_secs(2)),
            "the backgrounded sleep must die with the validation process group"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn returns_partial_output_while_an_escaped_child_keeps_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let pid_file = tmp.path().join("writer.pid");
        let command = format!(
            "printf early; setsid sh -c 'echo $$ > \"{0}\"; while :; do echo payload; done' & \
             while [ ! -s '{0}' ]; do sleep 0.05; done",
            pid_file.display()
        );

        let started = Instant::now();
        let output = run_shell_command_with_shutdown(
            &command,
            tmp.path(),
            &[],
            Duration::from_secs(30),
            || false,
        );
        let elapsed = started.elapsed();
        let writer = crate::runner::pipes::read_pid(&pid_file);
        // Nothing reads the abandoned pipe any more, so the escaped writer
        // gets EPIPE instead of filling an unreachable buffer.
        let writer_stopped =
            crate::runner::pipes::process_exits_within(writer, Duration::from_secs(2));
        unsafe {
            libc::kill(writer as libc::pid_t, libc::SIGKILL);
        }

        let output = output.unwrap();
        assert!(output.status.success());
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
        assert!(output.stdout.starts_with(b"early"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("still held open"));
        assert!(writer_stopped, "the escaped writer kept a live reader");
    }

    #[test]
    fn timeout_kills_the_validation_command_process_group() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("timeout-marker");
        let command = format!(
            "(sleep 1; printf timed_out > '{}') & wait",
            marker.display()
        );
        let started = Instant::now();
        let error = run_shell_command_with_shutdown(
            &command,
            tmp.path(),
            &[],
            std::time::Duration::from_millis(150),
            || false,
        )
        .unwrap_err();

        assert!(error.to_string().contains("timed out after"));
        assert!(started.elapsed() >= std::time::Duration::from_millis(150));
        assert!(
            error.to_string().contains("timed out after"),
            "timeout error should identify elapsed timeout"
        );
        assert!(
            error.to_string().contains(&command),
            "timeout error should identify the command"
        );
        std::thread::sleep(Duration::from_millis(250));
        assert!(
            !marker.exists(),
            "the validation command's background child must die with its process group"
        );
    }
}
