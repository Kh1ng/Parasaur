//! WSL may inherit a Windows npm wrapper before Linux npm. That wrapper runs
//! node.exe even when `node` on PATH is Linux, so it cannot install a Unix
//! release runtime reliably. Ordinary Unix npm and fixture commands stay intact.
use anyhow::Result;
use std::process::Command;

pub(super) fn command() -> Result<Command> {
    #[cfg(unix)]
    {
        use anyhow::{ensure, Context};
        use std::os::unix::fs::PermissionsExt;
        use std::path::PathBuf;
        let path = std::env::var_os("PATH").unwrap_or_default();
        let selected = std::env::split_paths(&path)
            .map(|dir| dir.join("npm"))
            .find(|file| {
                file.metadata()
                    .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            });
        let windows = selected
            .and_then(|file| file.canonicalize().ok())
            .is_some_and(|file| {
                file.parent()
                    .is_some_and(|dir| dir.join("node.exe").is_file())
            });
        if windows {
            let output = Command::new("node")
                .args(["-p", "process.execPath"])
                .output()
                .context("Windows npm is on PATH; install Linux Node.js with its matching npm")?;
            ensure!(
                output.status.success(),
                "Cannot resolve Linux Node.js for npm"
            );
            let node = PathBuf::from(String::from_utf8(output.stdout)?.trim());
            ensure!(
                node.is_absolute() && node.is_file(),
                "Windows npm is on PATH; Linux Node.js is required"
            );
            let directory = node
                .parent()
                .context("Node.js has no installation directory")?;
            let npm = directory.join("npm");
            ensure!(
                npm.is_file() && !directory.join("node.exe").is_file(),
                "Install npm alongside Linux Node.js; refusing Windows npm for a Unix update"
            );
            let mut command = Command::new(npm);
            // Lifecycle scripts must see the same Node runtime as npm itself.
            command.env(
                "PATH",
                std::env::join_paths(
                    std::iter::once(directory.to_path_buf()).chain(std::env::split_paths(&path)),
                )?,
            );
            return Ok(command);
        }
    }
    Ok(Command::new("npm"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test_support::{ExecGuard, PathGuard};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn executable(path: &Path, body: &str) {
        fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn windows_npm_is_replaced_by_linux_nodes_sibling_and_lifecycle_path() {
        let _exec = ExecGuard::new();
        let root = tempfile::tempdir().unwrap();
        let windows = root.path().join("windows");
        let probe = root.path().join("probe");
        let linux = root.path().join("linux");
        for path in [&windows, &probe, &linux] {
            fs::create_dir(path).unwrap();
        }
        fs::write(windows.join("node.exe"), "Windows runtime marker").unwrap();
        executable(&windows.join("npm"), "exit 97");
        executable(
            &probe.join("node"),
            &format!("printf '%s\\n' '{}'", linux.join("node").display()),
        );
        executable(&linux.join("node"), "printf 'linux-runtime\\n'");
        executable(
            &linux.join("npm"),
            "node -p process.platform; printf '%s\\n' \"$@\"",
        );
        let _path = PathGuard::set(std::env::join_paths([&windows, &probe]).unwrap());
        let output = command().unwrap().arg("ci").output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "linux-runtime\nci\n"
        );
    }

    #[test]
    fn ordinary_unix_npm_is_preserved_and_missing_linux_pair_fails_closed() {
        let _exec = ExecGuard::new();
        let root = tempfile::tempdir().unwrap();
        executable(&root.path().join("npm"), "printf 'selected-npm\\n'");
        let _path = PathGuard::set(root.path());
        let output = command().unwrap().output().unwrap();
        assert_eq!(output.stdout, b"selected-npm\n");
        fs::write(root.path().join("node.exe"), "Windows runtime marker").unwrap();
        executable(&root.path().join("node"), "exit 1");
        assert!(command().is_err(), "never fall back to the Windows wrapper");
    }
}
