//! Run credentials on the selected WSL node, without shell-interpolating fields
//! or persisting stdin secrets in Windows temporary files.
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) fn invoke(
    args: Vec<String>,
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let command = command(&super::super::read_settings(), args, timeout);
    // Give WSL's Linux process-group timeout time to reap the CLI before the
    // outer bound kills a stuck Windows transport (including WSL startup).
    run(command, input, timeout + Duration::from_secs(3)).map_err(|_| super::FAILURE.to_owned())
}

fn command(
    settings: &super::super::DesktopSettings,
    args: Vec<String>,
    timeout: Duration,
) -> Command {
    let mut command = super::super::wsl_command(settings);
    command.args([
        "--exec",
        "/usr/bin/timeout",
        "--kill-after=1s",
        &format!("{}s", timeout.as_secs_f64()),
        "/bin/sh",
        "-c",
        "exec \"$HOME/.cargo/bin/gah\" \"$@\"",
        "gah-credentials",
    ]);
    command.args(args);
    command
}

pub(super) fn run(
    command: Command,
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<Vec<u8>, ()> {
    tauri::async_runtime::block_on(async {
        let mut command = tokio::process::Command::from(command);
        command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| ())?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or(())?;
        let result = tokio::time::timeout(timeout, async {
            let write = async {
                if let (Some(mut stdin), Some(input)) = (stdin, input) {
                    stdin.write_all(input).await.map_err(|_| ())?;
                    stdin.shutdown().await.map_err(|_| ())?;
                }
                Ok::<_, ()>(())
            };
            let read = async {
                let mut bytes = Vec::new();
                stdout
                    .take(262145)
                    .read_to_end(&mut bytes)
                    .await
                    .map_err(|_| ())?;
                if bytes.len() > 262144 {
                    return Err(());
                }
                Ok(bytes)
            };
            let wait = async { child.wait().await.map_err(|_| ()) };
            let (_, bytes, status) = tokio::try_join!(write, read, wait)?;
            if !status.success() {
                return Err(());
            }
            Ok(bytes)
        })
        .await;
        match result {
            Ok(Ok(bytes)) => Ok(bytes),
            _ => {
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(1), child.wait()).await;
                Err(())
            }
        }
    })
}

#[cfg(test)]
mod tests;
