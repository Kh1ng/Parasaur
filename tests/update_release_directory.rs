#![cfg(target_os = "linux")]
use assert_cmd::Command;
use predicates::str::contains;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

fn write(root: &Path, path: &str, body: &str) {
    let dest = root.join(path);
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    fs::write(dest, body).unwrap();
}

#[test]
fn update_installed_control_plane_from_a_release_without_a_checkout() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("controller");
    let home = temp.path().join("home");
    let feed = temp.path().join("feed");
    let bundle = temp.path().join("bundle");
    for (path, text) in [
        ("package.json", r#"{"name":"git-agent-harness"}"#),
        ("package-lock.json", "same lockfile"),
        ("config/operator", "keep"),
        ("apps/server/dist/bin.js", "old API"),
        ("apps/web/dist/index.html", "old UI"),
        ("bin/gah", "old CLI"),
    ] {
        write(&root, path, text);
    }
    write(&home, "cargo/bin/gah", "old loop");
    write(&bundle, "package-lock.json", "same lockfile");
    write(&bundle, "apps/server/dist/bin.js", "new API");
    write(&bundle, "apps/web/dist/index.html", "new UI");
    write(&feed, "gah-cli", "#!/bin/sh\necho 'gah 0.3.0'\n");
    write(&feed, "gah-mcp", "#!/bin/sh\necho 'MCP 0.3.0'\n");
    let archive = feed.join("server.tar.gz");
    assert!(std::process::Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&bundle)
        .arg(".")
        .status()
        .unwrap()
        .success());
    let target = format!("linux-{}", std::env::consts::ARCH);
    let mut manifest = json!({"schema":1, "channel":"test", "version":"1.0.0", "assets":[]});
    for (name, kind) in [
        ("gah-cli", "cli"),
        ("gah-mcp", "mcp-server"),
        ("server.tar.gz", "server-bundle"),
    ] {
        let hash = format!("{:x}", Sha256::digest(fs::read(feed.join(name)).unwrap()));
        manifest["assets"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":name, "kind":kind, "target":target, "sha256":hash}));
    }
    let path = feed.join("edge-manifest.json");
    let cmd = || {
        let mut cmd = Command::cargo_bin("gah").unwrap();
        cmd.current_dir(&root)
            .env("HOME", &home)
            .env("CARGO_HOME", home.join("cargo"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("PARASAUR_TEST_EFFECTIVE_UID", "1000")
            .env("PARASAUR_BINARY", root.join("bin/gah"))
            .env("PARASAUR_RELEASE_MANIFEST", &path)
            .env("PARASAUR_WEB_DEPLOY_ROOT", "")
            .args(["update", "--role", "central", "--from-release"]);
        cmd
    };
    let mut bad = manifest.clone();
    bad["assets"][2]["sha256"] = json!("0".repeat(64));
    fs::write(&path, bad.to_string()).unwrap();
    cmd()
        .assert()
        .failure()
        .stderr(contains("checksum mismatch"));
    assert_eq!(
        fs::read_to_string(home.join("cargo/bin/gah")).unwrap(),
        "old loop"
    );
    assert_eq!(fs::read_to_string(root.join("bin/gah")).unwrap(), "old CLI");
    assert_eq!(
        fs::read_to_string(root.join("apps/server/dist/bin.js")).unwrap(),
        "old API"
    );
    fs::write(&path, manifest.to_string()).unwrap();
    cmd().assert().success();
    for cli in [
        root.join("bin/gah"),
        home.join("cargo/bin/gah"),
        home.join("cargo/bin/parasaur"),
    ] {
        assert_eq!(
            fs::read(cli).unwrap(),
            fs::read(feed.join("gah-cli")).unwrap()
        );
    }
    assert_eq!(
        fs::read(home.join("cargo/bin/gah-mcp-server")).unwrap(),
        fs::read(feed.join("gah-mcp")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(root.join("apps/server/dist/bin.js")).unwrap(),
        "new API"
    );
    assert_eq!(
        fs::read_to_string(root.join("apps/web/dist/index.html")).unwrap(),
        "new UI"
    );
    assert_eq!(
        fs::read_to_string(root.join("config/operator")).unwrap(),
        "keep"
    );
    assert!(!home.join("config/systemd").exists());
    // Older installations point the loop command at the server's copy.
    // A subsequent upgrade must refresh its referent and preserve the link.
    let loop_cli = home.join("cargo/bin/gah");
    fs::remove_file(&loop_cli).unwrap();
    std::os::unix::fs::symlink(root.join("bin/gah"), &loop_cli).unwrap();
    write(&root, "bin/gah", "older linked CLI");
    cmd().assert().success();
    assert!(loop_cli.is_symlink());
    assert_eq!(
        fs::read(&loop_cli).unwrap(),
        fs::read(feed.join("gah-cli")).unwrap()
    );
}
