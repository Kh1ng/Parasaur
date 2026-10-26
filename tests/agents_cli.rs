#![cfg(unix)]
use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn fake(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn gah(dir: &Path) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("gah"));
    command
        .env("PATH", dir)
        .env("HOME", dir)
        .env("XDG_STATE_HOME", dir.join("state"));
    command
}
#[test]
fn agents_report_all_backends_and_probe_registry_without_updating() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["claude", "codex", "agy", "vibe", "opencode"] {
        fake(
            dir.path(),
            name,
            "[ \"$1\" = --version ] || exit 9\necho 'cli 1.0.0'",
        );
    }
    fake(
        dir.path(),
        "npm",
        "[ \"$1\" = view ] || exit 9\necho '\"1.2.0\"'",
    );
    fake(
        dir.path(),
        "curl",
        "echo '{\"version\":\"1.2.0\",\"info\":{\"version\":\"1.2.0\"}}'",
    );
    let output = gah(dir.path())
        .args(["agents", "versions", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<Value> = serde_json::from_slice(&output).unwrap();
    assert_eq!(rows.len(), 7);
    for row in rows.iter().filter(|r| r["installed"].is_string()) {
        assert_eq!(row["latest"], "1.2.0");
        assert_eq!(row["outdated"], true);
    }
    assert!(rows.iter().find(|r| r["backend"] == "hermes").unwrap()["error"].is_string());
    assert!(!dir.path().join("state").exists());
}
#[test]
fn agents_update_requires_consent_and_persists_success_and_failure() {
    let dir = tempfile::tempdir().unwrap();
    fake(
        dir.path(),
        "claude",
        "if [ \"$1\" = --version ]; then echo '2.0.0'; else echo updated > \"$HOME/updated\"; fi",
    );
    gah(dir.path())
        .args(["agents", "update", "claude"])
        .assert()
        .failure();
    assert!(!dir.path().join("updated").exists());
    gah(dir.path())
        .args(["agents", "update", "claude", "--yes"])
        .assert()
        .success();
    let path = dir.path().join("state/gah/agent-cli-updates/claude.json");
    let record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(record["status"], "success");
    assert_eq!(record["after"], "2.0.0");
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fake(
        dir.path(),
        "claude",
        "if [ \"$1\" = --version ]; then echo '2.0.0'; else exit 42; fi",
    );
    gah(dir.path())
        .args(["agents", "update", "claude", "--yes"])
        .assert()
        .failure();
    let record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(record["status"], "failed");
    gah(dir.path())
        .args(["agents", "update", "unknown", "--yes"])
        .assert()
        .failure();
}
#[test]
fn agents_hung_probes_have_a_ten_second_deadline() {
    let dir = tempfile::tempdir().unwrap();
    fake(dir.path(), "codex", "/bin/sleep 60");
    let start = std::time::Instant::now();
    let output = gah(dir.path())
        .args(["agents", "versions", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(start.elapsed().as_secs() < 15);
    let rows: Vec<Value> = serde_json::from_slice(&output).unwrap();
    assert!(
        rows.iter().find(|r| r["backend"] == "codex").unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("10 seconds")
    );
}

#[test]
fn agents_update_lock_prevents_concurrent_installers() {
    use fs2::FileExt;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("state/gah/agent-cli-updates");
    fs::create_dir_all(&folder).unwrap();
    let lock = fs::File::create(folder.join("update.lock")).unwrap();
    lock.lock_exclusive().unwrap();
    fake(dir.path(), "claude", "echo '1.0.0'");
    gah(dir.path())
        .args(["agents", "update", "claude", "--yes"])
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "Another node or agent CLI update is running",
        ));
    assert!(!folder.join("claude.json").exists());
}

#[test]
fn agents_update_uses_npm_for_an_npm_codex_installation() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("node_modules/@openai/codex/bin");
    fs::create_dir_all(&package).unwrap();
    fake(
        &package,
        "codex",
        "[ \"$1\" = --version ] || exit 9\necho '0.120.0'",
    );
    std::os::unix::fs::symlink(package.join("codex"), dir.path().join("codex")).unwrap();
    fake(dir.path(), "npm", "[ \"$*\" = 'install --global @openai/codex@latest' ] || exit 9\necho yes > \"$HOME/npm-updated\"");
    gah(dir.path())
        .args(["agents", "update", "codex", "--yes"])
        .assert()
        .success();
    assert!(dir.path().join("npm-updated").exists());
}
