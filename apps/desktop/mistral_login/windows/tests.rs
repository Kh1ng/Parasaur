use super::*;

#[test]
#[ignore = "read-only preflight against the actual selected WSL installation"]
fn installed_wsl_mistral_preflight_is_available() {
    assert_eq!(
        std::env::var("PARASAUR_WINDOWS_WSL_TEST_INSTALLED_READS").as_deref(),
        Ok("1")
    );
    assert!(installed());
    if let Err(reply) = preflight(&LoginTarget::default()) {
        assert_eq!(reply.message, super::super::custom_cookie_status().message);
    }
}

#[test]
#[ignore = "requires PARASAUR_WINDOWS_WSL_TEST_DISTRIBUTION; uses a synthetic WSL collector"]
fn windows_staging_preserves_connections_until_verification() {
    let settings = crate::DesktopSettings {
        wsl_distribution: std::env::var("PARASAUR_WINDOWS_WSL_TEST_DISTRIBUTION")
            .expect("WSL distribution required"),
        ..Default::default()
    };
    let setup = r#"import json,os,pathlib,sys,tempfile,uuid
os.umask(0o077)
p=pathlib.Path(tempfile.mkdtemp(prefix='parasaur-mistral-windows-'))
(p/'.cargo/bin').mkdir(parents=True); (p/'.config/gah').mkdir(parents=True)
(p/'.cargo/bin/gah').write_bytes(sys.stdin.buffer.read()); (p/'.cargo/bin/gah').chmod(0o700)
(p/'mode').write_text('good'); (p/'saved.json').write_text(json.dumps({'work':'old-work','sibling':'old-sibling'}))
(p/'.config/gah/mistral-dashboard.cookie').write_text('old-default')
(p/'custom.cookie').write_text('old-custom')
(p/'versions.json').write_text(json.dumps({'work':str(uuid.uuid4()),'':str(uuid.uuid4())}))
print(p)
"#;
    let mut prepare = crate::wsl_command(&settings);
    prepare.args(["--exec", "/usr/bin/python3", "-c", setup]);
    let bytes = run(
        prepare,
        Some(include_bytes!("test_cli.py")),
        Duration::from_secs(10),
    )
    .unwrap();
    let root = String::from_utf8(bytes).unwrap().trim().to_owned();
    assert!(root.starts_with("/tmp/parasaur-mistral-windows-"));
    let control = |code: &str, extra: &[&str]| {
        let mut command = crate::wsl_command(&settings);
        command
            .args(["--exec", "/usr/bin/python3", "-c", code, &root])
            .args(extra);
        run(command, None, Duration::from_secs(10)).unwrap()
    };
    let named = LoginTarget::new(
        Some("work".into()),
        Some("Work \"literal\" & account".into()),
    )
    .unwrap();
    let default = LoginTarget::default();
    let isolated_command = |target: &LoginTarget, probe: bool, custom: bool| {
        let original = check_command(&settings, target, probe);
        let args: Vec<_> = original.get_args().map(std::ffi::OsStr::to_owned).collect();
        let split = args.iter().position(|arg| arg == "--exec").unwrap() + 1;
        let mut command = crate::command(original.get_program().to_str().unwrap());
        command.args(&args[..split]).args([
            "/usr/bin/env",
            "-i",
            &format!("HOME={root}"),
            "PATH=/usr/bin:/bin",
        ]);
        if custom {
            command.arg(format!(
                "MISTRAL_DASHBOARD_COOKIE_FILE={root}/custom.cookie"
            ));
        }
        command.args(&args[split..]);
        command
    };
    let execute = |target: &LoginTarget, probe: bool, custom: bool| {
        let bytes = run(
            isolated_command(target, probe, custom),
            (!probe).then_some(b"session=synthetic-new".as_slice()),
            Duration::from_secs(15),
        )
        .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["result"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(execute(&default, true, false), "ready");
    assert_eq!(execute(&default, true, true), "custom_source");
    assert_eq!(execute(&named, true, true), "ready");
    control(
        "import pathlib,sys; (pathlib.Path(sys.argv[1])/'mode').write_text('unsupported')",
        &[],
    );
    assert_eq!(execute(&named, true, false), "unsupported");
    for (mode, expected) in [
        ("auth", "auth_required"),
        ("collector", "collector"),
        ("invalid", "invalid_observation"),
        ("oversized", "invalid_observation"),
    ] {
        control(
            "import pathlib,sys; (pathlib.Path(sys.argv[1])/'mode').write_text(sys.argv[2])",
            &[mode],
        );
        for target in [&named, &default] {
            assert_eq!(execute(target, false, false), expected);
        }
        control("import json,pathlib,sys; p=pathlib.Path(sys.argv[1]); assert json.loads((p/'saved.json').read_text())=={'work':'old-work','sibling':'old-sibling'}; assert (p/'.config/gah/mistral-dashboard.cookie').read_text()=='old-default'; assert not list((p/'.config/gah').glob('.mistral-check-*'))", &[]);
    }
    control(
        "import pathlib,sys; (pathlib.Path(sys.argv[1])/'mode').write_text('save')",
        &[],
    );
    assert_eq!(execute(&named, false, false), "save");
    control("import json,pathlib,sys; p=pathlib.Path(sys.argv[1]); assert json.loads((p/'saved.json').read_text())['work']=='old-work'; (p/'mode').write_text('good')", &[]);
    for (target, selected) in [(&named, "work"), (&default, "")] {
        control("import pathlib,sys; p=pathlib.Path(sys.argv[1]); (p/'mutation').write_text(sys.argv[2]); (p/'mode').write_text('changed')", &[selected]);
        assert_eq!(execute(target, false, false), "changed");
    }
    control("import json,pathlib,sys; p=pathlib.Path(sys.argv[1]); assert json.loads((p/'saved.json').read_text())=={'work':'session=newer-work','sibling':'old-sibling'}; assert (p/'.config/gah/mistral-dashboard.cookie').read_text()=='session=newer-default'; (p/'saved.json').write_text(json.dumps({'work':'old-work','sibling':'old-sibling'})); (p/'.config/gah/mistral-dashboard.cookie').write_text('old-default'); (p/'mode').write_text('good')", &[]);
    assert_eq!(execute(&named, false, true), "connected");
    control("import json,pathlib,sys; p=pathlib.Path(sys.argv[1]); assert json.loads((p/'saved.json').read_text())=={'work':'session=synthetic-new','sibling':'old-sibling'}; assert (p/'.config/gah/mistral-dashboard.cookie').read_text()=='old-default'; assert (p/'custom.cookie').read_text()=='old-custom'; assert (p/'refreshed').read_text()=='work'", &[]);
    assert_eq!(execute(&default, false, false), "connected");
    control("import pathlib,sys; p=pathlib.Path(sys.argv[1]); assert (p/'.config/gah/mistral-dashboard.cookie').read_text()=='session=synthetic-new'; assert (p/'published').read_text()=='yes'; (p/'mode').write_text('usage')", &[]);
    assert_eq!(execute(&named, false, false), "usage");
    control(
        "import pathlib,sys; (pathlib.Path(sys.argv[1])/'mode').write_text('publish')",
        &[],
    );
    assert_eq!(execute(&default, false, false), "publish");
    if let Ok(gah) = std::env::var("PARASAUR_WINDOWS_WSL_TEST_GAH") {
        control("import pathlib,sys; p=pathlib.Path(sys.argv[1]); (p/'real-cli').write_text(sys.argv[2]); (p/'mode').write_text('good')", &[&gah]);
        assert_eq!(execute(&named, false, false), "connected");
        assert_eq!(execute(&default, false, false), "connected");
        for (target, selected) in [(&named, "work"), (&default, "")] {
            control("import pathlib,sys; p=pathlib.Path(sys.argv[1]); (p/'mutation').write_text(sys.argv[2]); (p/'mode').write_text('changed')", &[selected]);
            assert_eq!(execute(target, false, false), "changed");
        }
        control("import json,pathlib,sys; p=pathlib.Path(sys.argv[1]); assert json.loads((p/'.config/gah/credentials/work.json').read_text())['secret']=='session=newer-work'; assert (p/'.config/gah/mistral-dashboard.cookie').read_text()=='session=newer-default'; (p/'real-cli').unlink()", &[]);
    }
    control(
        "import pathlib,sys; (pathlib.Path(sys.argv[1])/'mode').write_text('hang')",
        &[],
    );
    let original = isolated_command(&named, false, false);
    let mut deadline = crate::command(original.get_program().to_str().unwrap());
    deadline.args(original.get_args().map(|arg| {
        if arg == "195s" {
            std::ffi::OsString::from("1s")
        } else {
            arg.to_owned()
        }
    }));
    assert!(run(
        deadline,
        Some(b"session=synthetic-new"),
        Duration::from_secs(4)
    )
    .is_err());
    let bytes = run(
        isolated_command(&named, false, false),
        Some(b"session=bad\nInjected: bad"),
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["result"],
        "invalid_cookie"
    );
    control(
        "import pathlib,sys; (pathlib.Path(sys.argv[1])/'.cargo/bin/gah').chmod(0o600)",
        &[],
    );
    assert_eq!(execute(&named, true, false), "not_installed");
    control("import pathlib,shutil,sys; p=pathlib.Path(sys.argv[1]); assert p.parent==pathlib.Path('/tmp') and p.name.startswith('parasaur-mistral-windows-'); assert not list((p/'.config/gah').glob('.mistral-check-*')); shutil.rmtree(p)", &[]);
}
