use super::*;
use std::io::{Read, Write};

#[test]
#[ignore = "child process fixture"]
fn transport_child() {
    match std::env::var("GAH_CREDENTIAL_TRANSPORT_MODE")
        .unwrap()
        .as_str()
    {
        "stdin" => {
            let mut value = String::new();
            std::io::stdin().read_to_string(&mut value).unwrap();
            assert_eq!(value, "synthetic-secret-only");
            assert!(std::env::args().all(|arg| !arg.contains(&value)));
            print!("metadata-only");
            std::io::stdout().flush().unwrap();
        }
        "overflow" => std::io::stdout().write_all(&vec![b'x'; 262145]).unwrap(),
        "wait" => std::thread::sleep(Duration::from_secs(30)),
        "fail" => std::process::exit(7),
        _ => panic!("unknown fixture"),
    }
    std::process::exit(0);
}

#[test]
fn pipes_keep_stdin_private_and_bound_output_failure_and_stalls() {
    let child = |mode| {
        let mut command = crate::command(std::env::current_exe().unwrap().to_str().unwrap());
        command
            .args([
                "--exact",
                "credentials::windows::tests::transport_child",
                "--ignored",
                "--nocapture",
            ])
            .env("GAH_CREDENTIAL_TRANSPORT_MODE", mode);
        command
    };
    let bytes = run(
        child("stdin"),
        Some(b"synthetic-secret-only"),
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(String::from_utf8(bytes).unwrap().ends_with("metadata-only"));
    assert!(run(child("overflow"), None, Duration::from_secs(5)).is_err());
    assert!(run(child("fail"), None, Duration::from_secs(5)).is_err());
    let started = std::time::Instant::now();
    assert!(run(
        child("wait"),
        Some(&vec![b'x'; 32768]),
        Duration::from_millis(100)
    )
    .is_err());
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
#[ignore = "requires GAH_WINDOWS_WSL_TEST_GAH and an installed WSL distribution"]
fn real_wsl_two_accounts_and_literal_arguments() {
    let gah = std::env::var("GAH_WINDOWS_WSL_TEST_GAH").expect("Linux fixture CLI path required");
    let distribution =
        std::env::var("GAH_WINDOWS_WSL_TEST_DISTRIBUTION").expect("WSL distribution required");
    let settings = crate::DesktopSettings {
        wsl_distribution: distribution,
        ..Default::default()
    };
    let root = format!(
        "/tmp/parasaur-windows-credentials-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let setup = r#"import os, pathlib, sys
os.umask(0o077)
root=pathlib.Path(sys.argv[1]); (root/'.cargo/bin').mkdir(parents=True)
(root/'.cargo/bin/gah').symlink_to(sys.argv[2])
(root/'config.toml').write_text('[defaults]\ncurrent_manager="codex"\n')
"#;
    let mut prepare = crate::wsl_command(&settings);
    prepare.args(["--exec", "/usr/bin/python3", "-c", setup, &root, &gah]);
    assert!(run(prepare, None, Duration::from_secs(15)).is_ok());
    let isolated_command = |args, timeout| {
        let command = super::command(&settings, args, timeout);
        let arguments: Vec<_> = command.get_args().map(std::ffi::OsStr::to_owned).collect();
        let split = arguments.iter().position(|arg| arg == "--exec").unwrap() + 1;
        let mut isolated = crate::command(command.get_program().to_str().unwrap());
        isolated
            .args(&arguments[..split])
            .args([
                "/usr/bin/env",
                "-i",
                &format!("HOME={root}"),
                "PATH=/usr/bin:/bin",
                &format!("GAH_CONFIG={root}/config.toml"),
            ])
            .args(&arguments[split..]);
        isolated
    };
    let execute = |args, input: Option<&[u8]>| {
        run(
            isolated_command(args, Duration::from_secs(10)),
            input,
            Duration::from_secs(13),
        )
    };
    execute(
        [
            "profile",
            "add",
            "test",
            "--display-name",
            "Test",
            "--repo-id",
            "owner/repo",
            "--provider",
            "github",
            "--repo",
            "owner/repo",
            "--local-path",
            &root,
            "--artifact-root",
            &format!("{root}/artifacts"),
        ]
        .map(str::to_owned)
        .into(),
        None,
    )
    .unwrap();
    let label = "Personal \"literal\" $(touch SHOULD_NOT_EXIST); & workspace";
    for (id, key) in [
        ("account-one", "synthetic-key-one"),
        ("account-two", "synthetic-key-two"),
    ] {
        let bytes = execute(
            super::super::cli_args::save(id, "openai", "api_key", label, Some("OPENAI_API_KEY")),
            Some(key.as_bytes()),
        )
        .unwrap();
        let metadata: super::super::CredentialInfo = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(metadata.id, id);
        assert_eq!(metadata.account_label, label);
        assert!(!String::from_utf8(bytes).unwrap().contains(key));
    }
    let bytes = execute(super::super::cli_args::list(), None).unwrap();
    let accounts: Vec<super::super::CredentialInfo> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(accounts.len(), 2);
    for (instance, id) in [("personal", "account-one"), ("work", "account-two")] {
        execute(
            super::super::cli_args::add_instance("test", instance, "codex", id),
            None,
        )
        .unwrap();
    }
    let initial: serde_json::Value =
        serde_json::from_slice(&execute(super::super::cli_args::instances(), None).unwrap())
            .unwrap();
    for (instance, id) in [("personal", "account-one"), ("work", "account-two")] {
        assert!(initial["profiles"]["test"]["backend_instances"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["backend_instance"] == instance && item["credential_id"] == id));
    }
    execute(
        super::super::cli_args::bind("test", "personal", "account-two"),
        None,
    )
    .unwrap();
    let catalog: serde_json::Value =
        serde_json::from_slice(&execute(super::super::cli_args::instances(), None).unwrap())
            .unwrap();
    let instances = catalog["profiles"]["test"]["backend_instances"]
        .as_array()
        .unwrap();
    assert!(instances
        .iter()
        .any(|instance| instance["backend_instance"] == "personal"
            && instance["credential_id"] == "account-two"));
    execute(super::super::cli_args::remove("account-one"), None).unwrap();
    let remaining: Vec<super::super::CredentialInfo> =
        serde_json::from_slice(&execute(super::super::cli_args::list(), None).unwrap()).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "account-two");
    assert!(execute(super::super::cli_args::refresh("missing-account"), None).is_err());
    let stall = r#"import pathlib,sys
p=pathlib.Path(sys.argv[1]); gah=p/'.cargo/bin/gah'; gah.unlink()
gah.write_text('#!/bin/sh\nsleep 30 &\nprintf "%s" "$!" > "$HOME/child.pid"\nwait\n'); gah.chmod(0o700)
"#;
    let mut prepare_stall = crate::wsl_command(&settings);
    prepare_stall.args(["--exec", "/usr/bin/python3", "-c", stall, &root]);
    assert!(run(prepare_stall, None, Duration::from_secs(10)).is_ok());
    assert!(run(
        isolated_command(super::super::cli_args::list(), Duration::from_millis(500)),
        None,
        Duration::from_secs(4)
    )
    .is_err());
    let mut cleanup = crate::wsl_command(&settings);
    cleanup.args(["--exec", "/usr/bin/python3", "-c", "import pathlib,shutil,sys; p=pathlib.Path(sys.argv[1]); assert p.parent==pathlib.Path('/tmp') and p.name.startswith('parasaur-windows-credentials-'); assert not (p/'SHOULD_NOT_EXIST').exists(); child=pathlib.Path('/proc')/(p/'child.pid').read_text()/'stat'; assert not child.exists() or child.read_text().split()[2]=='Z'; shutil.rmtree(p)", &root]);
    assert!(run(cleanup, None, Duration::from_secs(10)).is_ok());
}
