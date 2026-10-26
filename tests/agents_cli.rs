#![cfg(unix)]
mod support;
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
    let config = dir.join("config.toml");
    if !config.exists() {
        fs::write(&config, "[defaults]\n").unwrap();
    }
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("gah"));
    command
        .env("GAH_CONFIG", config)
        .env("GAH_CLAIM_STATE_PATH", dir.join("claims.json"))
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
        "[ \"$1\" = view ] || exit 9\nprintf '{\"name\":\"%s\",\"version\":\"1.2.0\"}' \"$2\"",
    );
    fake(
        dir.path(),
        "curl",
        &format!(
            "echo '{}'",
            serde_json::json!({"version":"1.2.0", "url":"https://example.org/agy", "sha512":"a".repeat(128), "info":{"name":"mistral-vibe", "version":"1.2.0"}})
        ),
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
        "npm",
        r#"echo '{"name":"@anthropic-ai/claude-code","version":"2.0.0"}'"#,
    );
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
    fake(dir.path(), "npm", "if [ \"$1\" = view ]; then echo '{\"name\":\"@openai/codex\",\"version\":\"0.121.0\"}'; exit; fi\n[ \"$*\" = 'install --global @openai/codex@latest' ] || exit 9\necho yes > \"$HOME/npm-updated\"");
    gah(dir.path())
        .args(["agents", "update", "codex", "--yes"])
        .assert()
        .success();
    assert!(dir.path().join("npm-updated").exists());
}

#[test]
fn agents_policy_refuses_read_only_and_unreadable_config_before_install() {
    let dir = tempfile::tempdir().unwrap();
    fake(
        dir.path(),
        "claude",
        "echo touched > \"$HOME/touched\"; echo 1.0.0",
    );
    let policy = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/policy-read-only.toml");
    fs::write(
        dir.path().join("config.toml"),
        format!(
            r#"
[profiles.readonly]
display_name = "Read only"
repo_id = "test"
provider = "github"
repo = "owner/repo"
local_path = "/tmp"
artifact_root = "/tmp"
default_target_branch = "main"
policy_path = {:?}
"#,
            policy
        ),
    )
    .unwrap();
    for args in [
        vec!["agents", "check-update-policy"],
        vec!["agents", "update", "claude", "--yes"],
    ] {
        gah(dir.path())
            .args(args)
            .assert()
            .failure()
            .stderr(predicates::str::contains("POLICY BLOCKED"));
    }
    assert!(!dir.path().join("touched").exists());
    fs::write(dir.path().join("config.toml"), "junk").unwrap();
    gah(dir.path())
        .args(["agents", "update", "claude", "--yes"])
        .assert()
        .failure();
    assert!(!dir.path().join("touched").exists());
}

#[test]
fn agents_installation_requires_fresh_valid_metadata() {
    let dir = tempfile::tempdir().unwrap();
    fake(
        dir.path(),
        "claude",
        "if [ \"$1\" = --version ]; then echo 1.0.0; else echo installed > \"$HOME/installed\"; fi",
    );
    for response in [
        "exit 22",
        "echo junk",
        r#"echo '{"name":"wrong","version":"2.0.0"}'"#,
        r#"echo '{"name":"@anthropic-ai/claude-code","version":"2.0.0;ignored"}'"#,
    ] {
        fake(dir.path(), "npm", response);
        let output = gah(dir.path())
            .args(["agents", "versions", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let rows: Vec<Value> = serde_json::from_slice(&output).unwrap();
        let claude = rows.iter().find(|r| r["backend"] == "claude").unwrap();
        assert!(claude["latest"].is_null());
        assert!(claude["error"].is_string());
        gah(dir.path())
            .args(["agents", "update", "claude", "--yes"])
            .assert()
            .failure()
            .stderr(predicates::str::contains("Latest release unknown"));
        assert!(!dir.path().join("installed").exists());
    }
    fake(
        dir.path(),
        "agy",
        "if [ \"$1\" = --version ]; then echo 1.0.0; else echo installed > \"$HOME/installed\"; fi",
    );
    fake(dir.path(), "curl", "exit 22");
    gah(dir.path())
        .args(["agents", "update", "agy", "--yes"])
        .assert()
        .failure();
    assert!(!dir.path().join("installed").exists());
}

#[test]
fn agents_dispatch_starting_during_lookup_prevents_installation() {
    let dir = tempfile::tempdir().unwrap();
    fake(
        dir.path(),
        "claude",
        "if [ \"$1\" = --version ]; then echo 1.0.0; else echo installed > \"$HOME/installed\"; fi",
    );
    fake(dir.path(), "kill", "exec /bin/kill \"$@\"");
    let claims = serde_json::json!({"version":2, "claims":{"other@repo":[{
        "work_id":"#7", "pid":std::process::id(), "hostname":hostname::get().unwrap().to_string_lossy(),
        "claimed_at":chrono::Utc::now().to_rfc3339()
    }]}});
    // Represents an already admitted dispatch becoming visible while metadata is in flight.
    fake(dir.path(), "npm", &format!("echo '{claims}' > \"$GAH_CLAIM_STATE_PATH\"\necho '{{\"name\":\"@anthropic-ai/claude-code\",\"version\":\"2.0.0\"}}'"));
    gah(dir.path())
        .args(["agents", "update", "claude", "--yes"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("active dispatches"));
    assert!(!dir.path().join("installed").exists());
}

// Separate process keeps admission tests off the runner's environment and locks.
#[test]
fn agents_claim_admission_child() {
    let Ok(expected) = std::env::var("GAH_TEST_CLAIM_ALLOWED") else {
        return;
    };
    let allowed = expected == "yes";
    assert_eq!(
        git_agent_harness::work_claim::try_claim_work("loop@repo", "#1").unwrap(),
        allowed
    );
    assert_eq!(
        git_agent_harness::work_claim::try_claim_manual_work("manual@repo", "#2").unwrap(),
        allowed
    );
}

#[test]
fn agents_installer_holds_dispatch_admission_until_exit() {
    let dir = tempfile::tempdir().unwrap();
    fake(dir.path(), "claude", "if [ \"$1\" = --version ]; then echo 1.0.0; else echo started > \"$HOME/started\"; while [ ! -f \"$HOME/finish\" ]; do /bin/sleep 0.01; done; fi");
    fake(
        dir.path(),
        "npm",
        r#"echo '{"name":"@anthropic-ai/claude-code","version":"2.0.0"}'"#,
    );
    let _ = gah(dir.path());
    use std::os::unix::process::CommandExt;
    let mut updater = support::ProcessGroupGuard::new(
        std::process::Command::new(assert_cmd::cargo::cargo_bin!("gah"))
            .env("GAH_CONFIG", dir.path().join("config.toml"))
            .env("HOME", dir.path())
            .env("PATH", dir.path())
            .env("XDG_STATE_HOME", dir.path().join("state"))
            .env("GAH_CLAIM_STATE_PATH", dir.path().join("claims.json"))
            .args(["agents", "update", "claude", "--yes"])
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !dir.path().join("started").exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let claim = |allowed| {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "agents_claim_admission_child", "--nocapture"])
            .env("XDG_STATE_HOME", dir.path().join("state"))
            .env("GAH_CLAIM_STATE_PATH", dir.path().join("claims.json"))
            .env("GAH_TEST_CLAIM_ALLOWED", allowed)
            .assert()
            .success();
    };
    claim("no");
    fs::write(dir.path().join("finish"), "yes").unwrap();
    assert!(updater.wait().unwrap().success());
    claim("yes");
}

#[test]
fn agents_https_registry_rejects_redirect_to_http() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::CommandExt;
    use std::process::{Command as Process, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let openssl = Process::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost",
            "-keyout",
        ])
        .arg(dir.path().join("key.pem"))
        .arg("-out")
        .arg(dir.path().join("cert.pem"))
        .output()
        .unwrap();
    assert!(
        openssl.status.success(),
        "{}",
        String::from_utf8_lossy(&openssl.stderr)
    );
    let child = Process::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agents/redirect_server.py"))
        .arg(dir.path())
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap();
    let mut server = support::ProcessGroupGuard::new(child);
    let mut port = String::new();
    BufReader::new(server.stdout.take().unwrap())
        .read_line(&mut port)
        .unwrap();
    let url = format!("https://localhost:{}/release", port.trim());
    // Change only the source URL and trust root, retaining production curl arguments.
    let curl = Process::new("sh")
        .args(["-c", "command -v curl"])
        .output()
        .unwrap();
    let curl = String::from_utf8(curl.stdout).unwrap();
    let python = Process::new("python3")
        .args(["-c", "import sys; print(sys.executable)"])
        .output()
        .unwrap();
    let python = String::from_utf8(python.stdout).unwrap();
    let wrapper = dir.path().join("curl.py");
    fs::write(&wrapper, format!("import os,sys\nos.execv({curl:?}, [{curl:?}, '--cacert', {cert:?}, *sys.argv[1:-1], {url:?}])\n",
        curl=curl.trim(), cert=dir.path().join("cert.pem").to_str().unwrap())).unwrap();
    fake(
        dir.path(),
        "curl",
        &format!("exec '{}' '{}' \"$@\"", python.trim(), wrapper.display()),
    );
    fake(
        dir.path(),
        "agy",
        "if [ \"$1\" = --version ]; then echo 1.0.0; else echo installed > \"$HOME/installed\"; fi",
    );
    // Establish that the trusted HTTPS fixture and redirect actually work without the restriction.
    let unrestricted = Process::new(curl.trim())
        .args([
            "--fail",
            "--silent",
            "--location",
            "--max-time",
            "5",
            "--cacert",
        ])
        .arg(dir.path().join("cert.pem"))
        .arg(&url)
        .output()
        .unwrap();
    assert!(unrestricted.status.success());
    assert!(dir.path().join("downgraded").exists());
    fs::remove_file(dir.path().join("downgraded")).unwrap();
    let output = gah(dir.path())
        .args(["agents", "versions", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let rows: Vec<Value> = serde_json::from_slice(&output).unwrap();
    let agy = rows.iter().find(|r| r["backend"] == "agy").unwrap();
    assert!(agy["latest"].is_null());
    assert!(agy["error"].is_string());
    gah(dir.path())
        .args(["agents", "update", "agy", "--yes"])
        .assert()
        .failure();
    assert!(!dir.path().join("downgraded").exists());
    assert!(!dir.path().join("installed").exists());
}
