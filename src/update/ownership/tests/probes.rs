use super::*;

// Observed with systemd 257.13, systemctl --user show -p User --value,
// on central on 2026-10-26. Each input replaced the throwaway unit before reload.
const SYSTEMD_USER_FIXTURES: &[(&str, &str)] = &[
    ("User=\"agent\"", "\"agent\""),
    ("User='agent'", "'agent'"),
    ("User= agent ", "agent"),
    ("User=\u{a0}agent\u{a0}", "\u{a0}agent\u{a0}"),
    ("User=agent # c", "agent # c"),
    ("User=ag\\\nent", "ag ent"),
    ("User=\\\nagent", "agent"),
    (r"User=ag\x65nt", r"ag\x65nt"),
    ("User=ag\\\n   ent", "ag    ent"),
    ("User=agent\\", "agent"),
    (r"User=agent\\", r"agent\\"),
    ("User=agent\\ \nother", r"agent\"),
    ("User=ag\\\n# ignored\n  ent", "ag   ent"),
    (r"User=ag\nnt", r"ag\nnt"),
];

#[test]
fn ownership_fallback_matches_systemd_user_fixtures() {
    let dir = ownership_tempdir();
    let target = dir.path().join("probe.service");
    for &(directive, raw) in SYSTEMD_USER_FIXTURES {
        let body = format!("[Service]\n{directive}\n");
        assert_eq!(unit_user(&body).as_deref(), Some(raw), "{directive:?}");
        std::fs::write(&target, body).unwrap();
        let previous = previous_account(&target);
        if UserName::parse(raw).is_ok() {
            assert!(
                matches!(previous.unwrap(), PreviousAccount::User(user) if user.as_str() == raw)
            );
        } else {
            let error = format!("{:#}", previous.unwrap_err());
            assert!(error.contains(target.to_str().unwrap()), "{error}");
            assert!(error.contains(&format!("{raw:?}")), "{error}");
        }
    }
}

#[cfg(unix)]
#[test]
fn ownership_var_symlink_anchor_refuses_within_one_second() {
    let dir = ownership_tempdir();
    let repo = dir.path().join("var-anchor");
    std::os::unix::fs::symlink("/var", &repo).unwrap();
    let start = std::time::Instant::now();
    let error = server_managed_paths(&repo, &dir.path().join("absent.toml")).unwrap_err();
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert!(format!("{error:#}").contains("boundary"), "{error:#}");
}

// Called by the existing child-process fixture with systemctl absent from PATH.
pub(super) fn assert_unknown_disk_accounts(target: &Path, drop_in: &Path) {
    let rendered = UserName::parse("agent").unwrap();
    for &(directive, raw) in SYSTEMD_USER_FIXTURES {
        if UserName::parse(raw).is_ok() {
            continue;
        }
        let body = format!("[Service]\n{directive}\n");
        for file in [drop_in, target] {
            std::fs::write(file, &body).unwrap();
            let error =
                super::super::effective_previous_account(target, "gah-server.service", &rendered)
                    .unwrap_err();
            let message = format!("{error:#}");
            assert!(message.contains(file.to_str().unwrap()), "{message}");
            assert!(message.contains(&format!("{raw:?}")), "{message}");
            std::fs::remove_file(drop_in).ok();
        }
    }
    std::fs::write(target, "[Service]\nUser=olduser\n").unwrap();
    std::fs::write(drop_in, "[Service]\nUser=agent\n").unwrap();
}

#[cfg(unix)]
#[test]
fn ownership_stalled_origin_probe_is_killed_within_one_second() {
    use std::os::unix::fs::PermissionsExt;
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let repo = fixture_repo(dir.path());
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let git = bin.join("git");
    let real_git = crate::runner::resolve::resolve_executable_on_path("git").unwrap();
    // Concurrent tests outside ExecGuard must keep using real Git.
    std::fs::write(&git, format!(
        "#!/bin/sh\nif [ \"$1\" = -C ] && [ \"$2\" = '{}' ] && [ \"$3\" = config ]; then exec /bin/sleep 30; fi\nexec '{}' \"$@\"\n",
        repo.canonicalize().unwrap().display(), real_git.display(),
    )).unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _path = crate::test_support::PathGuard::set(bin.to_str().unwrap());
    let start = std::time::Instant::now();
    let error = server_managed_paths(&repo, &dir.path().join("absent.toml")).unwrap_err();
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert!(
        format!("{error:#}").contains("origin repository"),
        "{error:#}"
    );
}

#[cfg(unix)]
#[test]
fn ownership_stalled_filesystem_probe_fails_closed_within_one_second() {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    let dir = ownership_tempdir();
    let fifo = dir.path().join("config.toml");
    let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let writer_path = fifo.clone();
    // Rescue an unbounded implementation too, so a regression fails, not hangs.
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(900));
        std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(writer_path)
            .unwrap()
            .write_all(b"[routing]\n")
            .unwrap();
    });
    let candidate = fifo.clone();
    let (done, finished) = std::sync::mpsc::channel();
    let start = std::time::Instant::now();
    let result = super::super::read_only_probe(&fifo, move || {
        let result = std::fs::read_to_string(candidate);
        done.send(()).unwrap();
        Ok(result?)
    });
    let elapsed = start.elapsed();
    writer.join().unwrap();
    finished
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert!(elapsed < std::time::Duration::from_secs(1));
    let error = format!("{:#}", result.unwrap_err());
    assert!(
        error.contains("timed out") && error.contains(fifo.to_str().unwrap()),
        "{error}"
    );
}

#[test]
fn ownership_unknown_drop_in_refuses_before_install_or_chown() {
    let _exec = crate::test_support::ExecGuard::new();
    for &(directive, raw) in SYSTEMD_USER_FIXTURES {
        if UserName::parse(raw).is_ok() {
            continue;
        }
        let dir = ownership_tempdir();
        let (repo, config, _) = repair_fixture(dir.path());
        let template = repo.join("packaging/systemd/gah-server.service");
        let rendered = std::fs::read_to_string(&template)
            .unwrap()
            .replace("User=@USER@", "User=agent");
        std::fs::write(template, rendered).unwrap();
        let units = dir.path().join("units");
        let drop_in = units.join("gah-server.service.d/override.conf");
        std::fs::create_dir_all(drop_in.parent().unwrap()).unwrap();
        let target = units.join("gah-server.service");
        let installed = "[Service]\nUser=agent\n";
        std::fs::write(&target, installed).unwrap();
        std::fs::write(&drop_in, format!("[Service]\n{directive}\n")).unwrap();
        let bin = unit_install_shims(&target);
        let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
        let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));
        let error = super::super::super::install_server_unit_template_into(
            &repo,
            "gah-server.service",
            &units,
            true,
        )
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains(drop_in.to_str().unwrap()), "{message}");
        assert!(message.contains(&format!("{raw:?}")), "{message}");
        assert!(!bin.path().join("sudo-argv.log").exists());
        assert_eq!(std::fs::read_to_string(target).unwrap(), installed);
    }
}
