use super::{
    account_change, confirmed_chown_target, previous_account, repair_or_refuse,
    server_managed_paths, unit_user, AccountChange, PreviousAccount, UserName,
};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::TempDir;

#[test]
fn unit_user_reads_only_service_assignments() {
    assert_eq!(
        unit_user("[Service]\n  User = agent  \n").as_deref(),
        Some("agent")
    );
    assert_eq!(
        unit_user("[Service]\nUser=first\nUser=second\n").as_deref(),
        Some("second")
    );
    assert_eq!(
        unit_user("[Service]\nUser=agent\nUser=\n").as_deref(),
        Some("")
    );
    assert_eq!(
        unit_user("[Unit]\nUser=ignored\n[Service]\nExecStart=node\n"),
        None
    );
}

#[test]
fn unit_user_reads_continued_assignments_and_skips_comment_blocks() {
    for unit in [
        "[Service]\nUser\\\n=agent\n",
        "[Service]\nUser \\\n = agent\n",
        "[Service]\nUser=\\\n agent\n",
        "[Service]\\\n\nUser=agent\n",
        "[Service]\nUser\\\n# ignored\\\n; ignored too\n =\\\n agent\n",
        "[Service]\nDescription=escaped\\\\\nUser=agent\n",
        "[Service]\nDescription=trailing whitespace\\ \nUser=agent\n",
        "\u{feff}[Service]\nUser\\\n=agent",
        "[Service]\nUser=agent\\",
    ] {
        assert_eq!(unit_user(unit).as_deref(), Some("agent"), "{unit:?}");
    }
    assert_eq!(
        unit_user("[Service]\nDescription=continued\\\nUser=ignored\n"),
        None
    );
}

#[test]
fn previous_account_distinguishes_missing_root_and_user_units() {
    let dir = ownership_tempdir();
    assert!(matches!(
        previous_account(&dir.path().join("absent.service")).unwrap(),
        PreviousAccount::NotInstalled
    ));
    for (unit, expected) in [
        ("[Service]\n", "root"),
        ("[Service]\nUser=root\n", "root"),
        ("[Service]\nUser=agent\n", "agent"),
    ] {
        let path = dir.path().join("test.service");
        std::fs::write(&path, unit).unwrap();
        let actual = previous_account(&path).unwrap();
        match actual {
            PreviousAccount::Root => assert_eq!(expected, "root"),
            PreviousAccount::User(user) => assert_eq!(user.as_str(), expected),
            PreviousAccount::NotInstalled => panic!("installed unit was missed"),
        }
    }
    assert!(
        previous_account(dir.path()).is_err(),
        "read failures are fatal"
    );
}

#[cfg(unix)]
#[test]
fn unreadable_installed_unit_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let dir = ownership_tempdir();
    let path = dir.path().join("private.service");
    std::fs::write(&path, "[Service]\nUser=agent\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0)).unwrap();
    // A root test runner can read mode-000 files. The ordinary-user path
    // verifies the actual EACCES returned by the filesystem.
    if unsafe { libc::geteuid() } != 0 {
        let error = previous_account(&path).unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
}

#[test]
fn account_change_uses_validated_names() {
    use PreviousAccount::{NotInstalled, Root, User};
    let user = |name| User(UserName::parse(name).unwrap());
    assert!(account_change(NotInstalled, Some("khing".into()))
        .unwrap()
        .is_none());
    assert!(account_change(user("khing"), Some("khing".into()))
        .unwrap()
        .is_none());
    let change = account_change(user("agent"), Some("khing".into()))
        .unwrap()
        .unwrap();
    assert_eq!(change.new.as_str(), "khing");
    for invalid in [
        "",
        "Agent",
        "0agent",
        "agent.name",
        "agent\nsecret",
        "agent:group",
        "a2345678901234567890123456789012345",
    ] {
        assert!(UserName::parse(invalid).is_err(), "accepted {invalid:?}");
    }
    for valid in ["root", "agent", "_agent-1", &"a".repeat(32)] {
        assert!(UserName::parse(valid).is_ok());
    }
    assert!(account_change(NotInstalled, Some("bad.name".into())).is_err());
    assert!(account_change(Root, Some("khing".into())).is_err());
}

#[test]
fn ownership_root_requires_an_explicit_rendered_root_account() {
    assert!(account_change(PreviousAccount::Root, Some("root".into()))
        .unwrap()
        .is_none());
    for rendered in [None, Some("khing".into())] {
        assert!(account_change(PreviousAccount::Root, rendered).is_err());
    }
    assert!(account_change(
        PreviousAccount::User(UserName::parse("agent").unwrap()),
        None
    )
    .is_err());
    let change = account_change(
        PreviousAccount::User(UserName::parse("agent").unwrap()),
        Some("root".into()),
    )
    .unwrap()
    .unwrap();
    assert_eq!(change.new.as_str(), "root");
}

// macOS's default temp directory is under the denied /private/var tree.
fn ownership_tempdir() -> TempDir {
    tempfile::Builder::new()
        .prefix(".gah-ownership-")
        .tempdir_in(env!("CARGO_MANIFEST_DIR"))
        .unwrap()
}

fn fixture_repo(dir: &Path) -> PathBuf {
    let repo = dir.join("repo");
    std::fs::create_dir_all(repo.join("packaging/systemd")).unwrap();
    std::fs::create_dir_all(repo.join("apps/server/dist")).unwrap();
    std::fs::create_dir_all(repo.join("config/push")).unwrap();
    std::fs::write(repo.join("apps/server/dist/bin.js"), "server").unwrap();
    std::fs::write(repo.join("config/registry-config.json"), "{}").unwrap();
    std::fs::write(repo.join("config/push/vapid.json"), "{}").unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/systemd/gah-server.service"),
        repo.join("packaging/systemd/gah-server.service"),
    )
    .unwrap();
    std::fs::create_dir_all(repo.join(".git/objects")).unwrap();
    std::fs::create_dir_all(repo.join(".git/refs/heads")).unwrap();
    std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        repo.join(".git/config"),
        "[core]\nbare = false\n[remote \"origin\"]\nurl = https://github.com/Kh1ng/git-agent-harness.git\n",
    ).unwrap();
    repo
}

static WEB_DEPLOY_ROOT_LOCK: Mutex<()> = Mutex::new(());
static GAH_CONFIG_LOCK: Mutex<()> = Mutex::new(());

/// Scoped, serialized overrides for the two env vars the managed-path
/// collection reads; both must be restored before a parallel test can
/// observe process state.
struct ManagedPathsEnvGuard {
    _web: std::sync::MutexGuard<'static, ()>,
    _config: std::sync::MutexGuard<'static, ()>,
    web: Option<std::ffi::OsString>,
    config: Option<std::ffi::OsString>,
}

impl ManagedPathsEnvGuard {
    fn set(web: Option<&str>, config: Option<&Path>) -> Self {
        let web_lock = WEB_DEPLOY_ROOT_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let config_lock = GAH_CONFIG_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let web_original = std::env::var_os("GAH_WEB_DEPLOY_ROOT");
        match web {
            Some(value) => std::env::set_var("GAH_WEB_DEPLOY_ROOT", value),
            None => std::env::remove_var("GAH_WEB_DEPLOY_ROOT"),
        }
        let config_original = std::env::var_os("GAH_CONFIG");
        match config {
            Some(path) => std::env::set_var("GAH_CONFIG", path),
            None => std::env::remove_var("GAH_CONFIG"),
        }
        Self {
            _web: web_lock,
            _config: config_lock,
            web: web_original,
            config: config_original,
        }
    }
}

impl Drop for ManagedPathsEnvGuard {
    fn drop(&mut self) {
        match self.web.take() {
            Some(value) => std::env::set_var("GAH_WEB_DEPLOY_ROOT", value),
            None => std::env::remove_var("GAH_WEB_DEPLOY_ROOT"),
        }
        match self.config.take() {
            Some(value) => std::env::set_var("GAH_CONFIG", value),
            None => std::env::remove_var("GAH_CONFIG"),
        }
    }
}

/// A minimal but fully valid profile: the config loader requires every
/// non-defaulted field, and the ownership repair reads the artifact
/// roots it declares.
fn profile_toml(name: &str, artifact_root: &str) -> String {
    format!(
        "[profiles.{name}]\ndisplay_name = \"{name}\"\nrepo_id = \"{name}\"\n\
             provider = \"github\"\nrepo = \"owner/{name}\"\nlocal_path = \"/tmp/{name}\"\n\
             artifact_root = \"{artifact_root}\"\ndefault_target_branch = \"main\"\n"
    )
}

#[test]
fn managed_paths_collect_state_roots_checkout_and_web_root() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let repo = fixture_repo(dir.path());
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let config = state.join("config.toml");
    std::fs::write(
        &config,
        "[defaults]\nartifact_root = \"/srv/gah\"\n".to_string()
            + &profile_toml("a", "/srv/gah/a")
            + &profile_toml("b", state.to_str().unwrap()),
    )
    .unwrap();
    let web = dir.path().join("www");
    std::fs::create_dir_all(&web).unwrap();

    let env = ManagedPathsEnvGuard::set(Some(web.to_str().unwrap()), Some(&config));
    let managed = server_managed_paths(&repo, &config).unwrap();
    assert_eq!(
        managed
            .paths
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        [
            PathBuf::from("/srv/gah"),
            state.clone(),
            config.clone(),
            repo.join("config"),
            repo.join("apps/server/dist"),
            web.clone(),
        ]
        .into_iter()
        .collect(),
        "the nested /srv/gah/a root must be covered by /srv/gah"
    );
    for anchor in [repo.canonicalize().unwrap(), state.canonicalize().unwrap()] {
        assert!(
            managed.anchors.contains(&anchor),
            "missing anchor {}",
            anchor.display()
        );
    }
    assert!(!managed.anchors.contains(&web.canonicalize().unwrap()));
    drop(env);
    // An explicitly-empty GAH_WEB_DEPLOY_ROOT opts out of the web root.
    let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));
    let managed = server_managed_paths(&repo, &config).unwrap();
    assert!(!managed.paths.contains(&web));
}

#[test]
fn malformed_config_fails_the_repair_rather_than_guessing_state_roots() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let repo = fixture_repo(dir.path());
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[defaults\nbroken").unwrap();
    let error = server_managed_paths(&repo, &config).unwrap_err();
    assert!(error.to_string().contains("state roots"), "{error:#}");
    // A missing config contributes no state roots but keeps the
    // checkout and dist paths; an empty web root keeps the legacy
    // /var/www/gah fallback out of the list on hosts that have it.
    let missing = dir.path().join("absent.toml");
    let _env = ManagedPathsEnvGuard::set(Some(""), None);
    let managed = server_managed_paths(&repo, &missing).unwrap();
    assert_eq!(
        managed.paths,
        vec![repo.join("config"), repo.join("apps/server/dist")]
    );
}

#[test]
fn chown_targets_must_stay_under_the_anchors() {
    let dir = ownership_tempdir();
    let inside = dir.path().join("inside");
    let outside = dir.path().join("outside");
    for path in [&inside, &outside] {
        std::fs::create_dir_all(path).unwrap();
    }
    let anchors = vec![inside.canonicalize().unwrap()];
    assert_eq!(confirmed_chown_target(&inside, &anchors), None);
    assert_eq!(confirmed_chown_target(&outside, &anchors), None);
    // A symlink that stays under the anchor resolves to its target; one
    // that escapes is refused, and a dangling or missing path is nothing
    // to repair.
    let link = dir.path().join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&inside, &link).unwrap();
    assert_eq!(confirmed_chown_target(&link, &anchors), None);
    let escape = dir.path().join("escape");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &escape).unwrap();
    assert_eq!(confirmed_chown_target(&escape, &anchors), None);
    assert_eq!(
        confirmed_chown_target(&dir.path().join("missing"), &anchors),
        None
    );
}

/// The refusal names both accounts and every managed path, and never
/// touches the installed unit.
#[test]
fn refusal_names_both_accounts_and_the_paths() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let repo = fixture_repo(dir.path());
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let web = dir.path().join("www");
    std::fs::create_dir_all(&web).unwrap();
    let config = state.join("config.toml");
    std::fs::write(
        &config,
        format!("[defaults]\nartifact_root = \"{}\"\n", state.display()),
    )
    .unwrap();
    let _env = ManagedPathsEnvGuard::set(Some(web.to_str().unwrap()), Some(&config));
    let managed = server_managed_paths(&repo, &config).unwrap();
    let change = AccountChange {
        previous: UserName::parse("agent").unwrap(),
        new: UserName::parse("khing").unwrap(),
    };
    let error = repair_or_refuse(&repo, &change, &managed, false).unwrap_err();
    let message = format!("{error:#}");
    for expected in [
        "User=agent",
        "User=khing",
        "--no-repair-ownership",
        &state.display().to_string(),
        &repo.join("config").display().to_string(),
        &repo.join("apps/server/dist").display().to_string(),
        &web.display().to_string(),
        "issue #1626",
    ] {
        assert!(message.contains(expected), "missing {expected}: {message}");
    }
}

/// Executable fakes for the unit-install tests: `sudo` records its argv
/// and "installs" by copying the staged unit into the units directory;
/// `systemctl` answers --version and daemon-reload; `node` keeps the
/// render hermetic (the tracked template needs a Node path).
fn unit_install_shims(unit_target: &Path) -> TempDir {
    let bin = ownership_tempdir();
    let sudo_log = bin.path().join("sudo-argv.log");
    for (name, body) in [
            (
                "sudo",
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = install ]; then cp \"$8\" '{}'; fi\nexit 0\n",
                    sudo_log.display(),
                    bin.path().join("steps.log").display(),
                    unit_target.display()
                )
                .as_str(),
            ),
            (
                "systemctl",
                format!(r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
if [ "$1" = --version ]; then echo 'systemd 255'; exit 0; fi
if [ "$1" = show ]; then
  printf 'preflight: %s\n' "$*" >> '{steps}'
  case "$3" in
    LoadState) if [ -f '{target}' ]; then echo loaded; else echo not-found; fi ;;
    FragmentPath) echo '{target}' ;;
    User) cat '{target}' '{target}.d/'*.conf 2>/dev/null | sed -n 's/^User=//p' | tail -1 ;;
    DropInPaths) for f in '{target}.d/'*.conf; do [ ! -f "$f" ] || printf '%s ' "$f"; done ;;
  esac
fi
exit 0
"#, target=unit_target.display(), log=bin.path().join("systemctl.log").display(), steps=bin.path().join("steps.log").display()).as_str(),
            ),
            ("node", "#!/bin/sh\nexit 0\n"),
        ] {
            let shim = bin.path().join(name);
            std::fs::write(&shim, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&shim).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&shim, perms).unwrap();
            }
        }
    bin
}

fn current_user() -> String {
    let output = std::process::Command::new("id")
        .arg("-un")
        .output()
        .unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// A fixture whose config declares one state root, for the install-seam
/// tests: the fixture repo, the config file, and the state dir itself.
fn repair_fixture(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let repo = fixture_repo(dir);
    let state = dir.join("state");
    std::fs::create_dir_all(state.join("sessions")).unwrap();
    std::fs::write(state.join("sessions/private.json"), "private").unwrap();
    let config = dir.join("config.toml");
    std::fs::write(
        &config,
        format!("[defaults]\nartifact_root = \"{}\"\n", state.display()),
    )
    .unwrap();
    (repo, config, state)
}

/// Issue #1626 acceptance: rendering the unit over an existing installed
/// unit with a different `User=` repairs ownership of every
/// server-managed path, printing the exact chown commands, before the
/// new unit replaces the old one.
#[test]
fn rendering_over_a_unit_with_a_different_user_repairs_ownership() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, state) = repair_fixture(dir.path());
    let units = dir.path().join("units");
    std::fs::create_dir_all(&units).unwrap();
    std::fs::write(
        units.join("gah-server.service"),
        "[Unit]\nDescription=old\n\n[Service]\nUser=agent\n",
    )
    .unwrap();
    let bin = unit_install_shims(&units.join("gah-server.service"));
    let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
    // An empty web deploy root keeps the legacy /var/www/gah fallback
    // out of the assertions on hosts that have one.
    let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));

    let target =
        super::super::install_server_unit_template_into(&repo, "gah-server.service", &units, true)
            .unwrap()
            .unwrap();
    assert_eq!(target, units.join("gah-server.service"));

    let record = std::fs::read_to_string(bin.path().join("sudo-argv.log")).unwrap();
    let user = current_user();
    for managed in [
        &state.join("sessions"),
        &repo.join("config"),
        &repo.join("apps/server/dist"),
    ] {
        let chown = format!(
            "chown -R -h -P -- {user} {}",
            managed.canonicalize().unwrap().display()
        );
        assert!(record.contains(&chown), "missing repair {chown}: {record}");
    }
    assert_eq!(
        record.matches("chown").count(),
        3,
        "exactly the state root, config/ and dist are repaired: {record}"
    );
    // The repair precedes the unit swap, so a chown failure aborts
    // before the old unit is replaced.
    let chown_at = record.find("chown").expect("chown ran");
    let install_at = record.find("install -o").expect("install ran");
    assert!(chown_at < install_at, "{record}");
    assert!(record.contains("daemon-reload"), "{record}");
    let steps = std::fs::read_to_string(bin.path().join("steps.log")).unwrap();
    let phases: Vec<_> = steps
        .lines()
        .map(|line| line.split_whitespace().next().unwrap())
        .collect();
    let repair_at = phases.iter().position(|phase| *phase == "chown").unwrap();
    assert!(phases[..repair_at]
        .iter()
        .all(|phase| *phase == "preflight:"));
    assert_eq!(
        &phases[repair_at..],
        ["chown", "chown", "chown", "install", "systemctl"]
    );
    // The installed unit now runs as the new account.
    let unit = std::fs::read_to_string(target).unwrap();
    assert!(unit.contains(&format!("\nUser={user}\n")), "{unit}");
    assert!(!unit.contains("User=agent"), "{unit}");
}

/// `--no-repair-ownership`: the account change is refused before the
/// installed unit is touched, naming both accounts and the paths.
#[test]
fn no_repair_flag_refuses_the_account_change_without_touching_the_unit() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, state) = repair_fixture(dir.path());
    let units = dir.path().join("units");
    std::fs::create_dir_all(&units).unwrap();
    std::fs::write(units.join("gah-server.service"), "[Service]\nUser=agent\n").unwrap();
    let bin = unit_install_shims(&units.join("gah-server.service"));
    let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
    let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));

    let error =
        super::super::install_server_unit_template_into(&repo, "gah-server.service", &units, false)
            .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("User=agent"), "{message}");
    assert!(
        message.contains(&format!("User={}", current_user())),
        "{message}"
    );
    assert!(
        message.contains(&state.display().to_string())
            && message.contains(&repo.join("config").display().to_string())
            && message.contains(&repo.join("apps/server/dist").display().to_string()),
        "the refusal must name the paths: {message}"
    );
    // Nothing ran: the installed unit keeps the old account.
    let record = std::fs::read_to_string(bin.path().join("sudo-argv.log")).unwrap_or_default();
    assert!(record.is_empty(), "no privileged step may run: {record}");
    assert_eq!(
        std::fs::read_to_string(units.join("gah-server.service")).unwrap(),
        "[Service]\nUser=agent\n"
    );
}

/// The same account needs no repair, and a first install (no previously
/// installed unit) has nothing to compare against: neither case chowns.
#[test]
fn matching_or_missing_previous_account_never_chowns() {
    let _exec = crate::test_support::ExecGuard::new();
    for (installed, drop_in) in [
        (Some(current_user()), false),
        (None, false),
        (Some(current_user()), true),
        (None, true),
    ] {
        let dir = ownership_tempdir();
        let (repo, config, _state) = repair_fixture(dir.path());
        let units = dir.path().join("units");
        std::fs::create_dir_all(&units).unwrap();
        if let Some(user) = &installed {
            std::fs::write(
                units.join("gah-server.service"),
                format!("[Service]\nUser={user}\n"),
            )
            .unwrap();
        }
        if drop_in {
            let path = units.join("gah-server.service.d/override.conf");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, format!("[Service]\nUser\\\n={}\n", current_user())).unwrap();
        }
        let bin = unit_install_shims(&units.join("gah-server.service"));
        let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
        let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));

        super::super::install_server_unit_template_into(&repo, "gah-server.service", &units, true)
            .unwrap()
            .unwrap();
        let record = std::fs::read_to_string(bin.path().join("sudo-argv.log")).unwrap();
        assert!(!record.contains("chown"), "{installed:?}: {record}");
        assert!(record.contains("install"), "{installed:?}: {record}");
    }
}

/// A managed path that is a symlink escaping the checkout and the state
/// roots is never chowned; the repair still covers the remaining paths.
#[test]
fn escaped_symlink_is_absent_from_a_running_repair() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, state) = repair_fixture(dir.path());
    std::fs::remove_dir_all(repo.join("config")).unwrap();
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, repo.join("config")).unwrap();
    let units = dir.path().join("units");
    std::fs::create_dir_all(&units).unwrap();
    std::fs::write(units.join("gah-server.service"), "[Service]\nUser=agent\n").unwrap();
    let bin = unit_install_shims(&units.join("gah-server.service"));
    let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
    let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));

    super::super::install_server_unit_template_into(&repo, "gah-server.service", &units, true)
        .unwrap()
        .unwrap();
    let record = std::fs::read_to_string(bin.path().join("sudo-argv.log")).unwrap();
    assert!(!record.contains(&outside.display().to_string()), "{record}");
    assert_eq!(record.matches("chown").count(), 2, "{record}");
    assert!(
        record.contains(&format!(
            "chown -R -h -P -- {} {}",
            current_user(),
            state.join("sessions").canonicalize().unwrap().display()
        )),
        "{record}"
    );
}

/// Issue #894 (moved next to the seam it exercises): `gah update`
/// reinstalls the system-level `gah-server.service` from the tracked
/// template on every run, rendered for this account (#1322).
#[test]
fn server_unit_template_is_installed_on_every_update() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, _state) = repair_fixture(dir.path());
    let units = dir.path().join("units");
    std::fs::create_dir_all(&units).unwrap();
    let bin = unit_install_shims(&units.join("gah-server.service"));
    let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
    let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));

    let target =
        super::super::install_server_unit_template_into(&repo, "gah-server.service", &units, true)
            .unwrap()
            .unwrap();
    assert!(target.ends_with("gah-server.service"));
    let record = std::fs::read_to_string(bin.path().join("sudo-argv.log")).unwrap();
    assert!(
        record.contains("install"),
        "expected sudo install: {record}"
    );
    assert!(record.contains("gah-server.service"), "{record}");
    assert!(record.contains("daemon-reload"), "{record}");
    assert!(
        !record.contains("chown"),
        "first install repairs nothing: {record}"
    );
    // #1322: the installed unit is rendered for this account, not copied.
    let unit = std::fs::read_to_string(&target).unwrap();
    for placeholder in ["@USER@", "@REPO@", "@CONFIG@", "@NODE@", "@PATH@"] {
        assert!(!unit.contains(placeholder), "{placeholder}: {unit}");
    }
    assert!(
        unit.contains(&format!("\nUser={}\n", current_user())),
        "{unit}"
    );
    assert!(unit.contains(&format!(
        "\nWorkingDirectory={}\n",
        repo.canonicalize().unwrap().display()
    )));
}

/// The production entry point targets /etc/systemd/system; the seam's
/// assertions above cover the render and repair behavior.
#[test]
fn production_server_unit_install_targets_etc_systemd() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, _state) = repair_fixture(dir.path());
    let bin = unit_install_shims(&dir.path().join("installed.service"));
    let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
    let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));

    let target = super::super::install_server_unit_template(&repo, "gah-server.service", true)
        .unwrap()
        .unwrap();
    assert!(target.ends_with("/etc/systemd/system/gah-server.service"));
    let record = std::fs::read_to_string(bin.path().join("sudo-argv.log")).unwrap();
    assert!(record.contains("/etc/systemd/system"), "{record}");
    assert!(record.contains("daemon-reload"), "{record}");
}

#[test]
fn server_unit_name_cannot_escape_systemd_directory() {
    let dir = ownership_tempdir();
    let repo = fixture_repo(dir.path());
    for name in [
        "/tmp/pwn.service",
        "../pwn.service",
        "nested/pwn.service",
        "not-a-unit",
    ] {
        assert!(
            super::super::install_server_unit_template_into(
                &repo,
                name,
                &dir.path().join("units"),
                true
            )
            .is_err(),
            "{name}"
        );
    }
}

#[cfg(unix)]
#[test]
fn ownership_never_authorizes_roots_from_resolved_candidates() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, state) = repair_fixture(dir.path());
    std::fs::remove_dir_all(&state).unwrap();
    std::os::unix::fs::symlink("/home", &state).unwrap();
    std::fs::remove_dir_all(repo.join("config")).unwrap();
    std::os::unix::fs::symlink("/", repo.join("config")).unwrap();
    let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));
    let error = server_managed_paths(&repo, &config).unwrap_err();
    assert!(error.to_string().contains("state boundary"));
    std::fs::remove_file(&state).unwrap();
    let managed = server_managed_paths(&repo, &config).unwrap();
    assert_eq!(confirmed_chown_target(&state, &managed.anchors), None);
    assert_eq!(
        confirmed_chown_target(&repo.join("config"), &managed.anchors),
        None
    );
    assert_eq!(
        confirmed_chown_target(Path::new("/"), &[PathBuf::from("/")]),
        None
    );
}

#[test]
fn ownership_web_environment_cannot_authorize_a_root() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, _) = repair_fixture(dir.path());
    let _env = ManagedPathsEnvGuard::set(Some("/"), Some(&config));
    assert!(server_managed_paths(&repo, &config).is_err());
}

#[cfg(unix)]
#[test]
fn ownership_protected_checkout_symlinks_refuse_before_mutation() {
    let _exec = crate::test_support::ExecGuard::new();
    for root in ["/", "/home", "/etc", "/private/etc", "/usr/local"] {
        // /private/etc exists on macOS; the shared policy test covers it on Linux.
        if !Path::new(root).exists() {
            continue;
        }
        let dir = ownership_tempdir();
        let repo = dir.path().join("checkout");
        std::os::unix::fs::symlink(root, &repo).unwrap();
        let config = dir.path().join("absent.toml");
        let bin = unit_install_shims(&dir.path().join("installed.service"));
        let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
        let _env = ManagedPathsEnvGuard::set(Some("/etc/ssh"), Some(&config));
        let change = AccountChange {
            previous: UserName::parse("agent").unwrap(),
            new: UserName::parse("khing").unwrap(),
        };
        let error = server_managed_paths(&repo, &config)
            .and_then(|managed| repair_or_refuse(&repo, &change, &managed, true))
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("checkout"), "{message}");
        assert!(
            message.contains(root)
                && message.contains(repo.canonicalize().unwrap().to_str().unwrap()),
            "{message}"
        );
        assert!(!bin.path().join("sudo-argv.log").exists());
    }
}

#[cfg(unix)]
#[test]
fn ownership_checkout_requires_git_entry_and_matching_origin_before_mutation() {
    let _exec = crate::test_support::ExecGuard::new();
    for (git_config, proof) in [
        (None, "missing .git directory or gitfile"),
        (
            Some("[remote \"origin\"]\nurl = https://github.com/other/repo.git\n"),
            "origin does not match expected repository",
        ),
        (
            Some("[remote \"origin\"]\nurl = github.com/kh1ng/git-agent-harness\n"),
            "origin does not match expected repository",
        ),
        (
            Some("[core]\nbare = false\n"),
            "cannot prove origin repository",
        ),
    ] {
        let dir = ownership_tempdir();
        let target = fixture_repo(dir.path());
        if let Some(config) = git_config {
            std::fs::write(target.join(".git/config"), config).unwrap();
        } else {
            std::fs::remove_dir_all(target.join(".git")).unwrap();
        }
        let repo = dir.path().join("checkout");
        std::os::unix::fs::symlink(&target, &repo).unwrap();
        let bin = unit_install_shims(&dir.path().join("installed.service"));
        let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
        let _env = ManagedPathsEnvGuard::set(Some("/etc/ssh"), None);
        let change = AccountChange {
            previous: UserName::parse("agent").unwrap(),
            new: UserName::parse("khing").unwrap(),
        };
        let before = std::fs::read(target.join("apps/server/dist/bin.js")).unwrap();
        let error = server_managed_paths(&repo, &dir.path().join("absent.toml"))
            .and_then(|managed| repair_or_refuse(&repo, &change, &managed, true))
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains(proof), "{message}");
        assert!(
            message.contains(target.canonicalize().unwrap().to_str().unwrap()),
            "{message}"
        );
        assert!(!bin.path().join("sudo-argv.log").exists());
        assert_eq!(
            std::fs::read(target.join("apps/server/dist/bin.js")).unwrap(),
            before
        );
    }
}

#[test]
fn ownership_denies_system_prefixes_but_allows_deployment_layouts() {
    for prefix in [
        "/etc",
        "/private/etc",
        "/private/var",
        "/usr",
        "/bin",
        "/sbin",
        "/System",
        "/Library",
        "/dev",
        "/proc",
        "/sys",
        "/boot",
        "/var/root",
    ] {
        assert!(super::protected_root(Path::new(prefix)), "{prefix}");
        assert!(
            super::protected_root(&Path::new(prefix).join("child/grandchild")),
            "{prefix}"
        );
    }
    for home in ["/Users/some-user", "/home/some-user"] {
        assert!(super::protected_root(Path::new(home)), "{home}");
        assert!(
            !super::protected_root(&Path::new(home).join("gah")),
            "{home}"
        );
    }
    for path in [
        "/srv/agent-lab/repos/github/Kh1ng/git-agent-harness",
        "/srv/gah/state",
        "/opt/gah",
        "/usr-local/gah",
    ] {
        assert!(!super::protected_root(Path::new(path)), "{path}");
    }
}

#[test]
fn ownership_proves_checkout_gitfiles_and_state_config_or_markers() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let original = fixture_repo(dir.path());
    let repo = dir
        .path()
        .join("srv/agent-lab/repos/github/Kh1ng/git-agent-harness");
    std::fs::create_dir_all(repo.parent().unwrap()).unwrap();
    std::fs::rename(original, &repo).unwrap();
    let state = dir.path().join("srv/gah/state");
    std::fs::create_dir_all(&state).unwrap();
    let config = state.join("unit-config.toml");
    let text = format!("[defaults]\nartifact_root = \"{}\"\n", state.display());
    std::fs::write(&config, &text).unwrap();
    let _env = ManagedPathsEnvGuard::set(Some(""), None);
    let managed = server_managed_paths(&repo, &config).unwrap();
    for root in [&repo, &state] {
        assert!(managed.anchors.contains(&root.canonicalize().unwrap()));
    }
    assert!(confirmed_chown_target(&repo.join("config"), &managed.anchors).is_some());
    assert!(confirmed_chown_target(&config, &managed.anchors).is_some());

    let git_dir = dir.path().join("git-metadata");
    std::fs::rename(repo.join(".git"), &git_dir).unwrap();
    std::fs::write(
        repo.join(".git"),
        format!("gitdir: {}\n", git_dir.display()),
    )
    .unwrap();
    for origin in [
        "git@github.com:Kh1ng/git-agent-harness.git",
        "ssh://git@github.com/Kh1ng/git-agent-harness.git",
    ] {
        std::fs::write(
            git_dir.join("config"),
            format!("[remote \"origin\"]\nurl = {origin}\n"),
        )
        .unwrap();
        assert!(server_managed_paths(&repo, &config).is_ok());
    }
    let external = dir.path().join("external-config.toml");
    std::fs::rename(&config, &external).unwrap();
    let error = server_managed_paths(&repo, &external).unwrap_err();
    assert!(
        error.to_string().contains("neither unit config"),
        "{error:#}"
    );
    assert!(
        error
            .to_string()
            .contains(state.canonicalize().unwrap().to_str().unwrap()),
        "{error:#}"
    );
    std::fs::write(state.join("events.jsonl"), "").unwrap();
    assert!(server_managed_paths(&repo, &external).is_ok());
}

#[cfg(unix)]
#[test]
fn ownership_state_system_prefix_and_symlink_marker_refuse_before_mutation() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, state) = repair_fixture(dir.path());
    let bin = unit_install_shims(&dir.path().join("installed.service"));
    let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
    let _env = ManagedPathsEnvGuard::set(Some(""), None);
    std::fs::remove_dir_all(state.join("sessions")).unwrap();
    std::os::unix::fs::symlink(&config, state.join("events.jsonl")).unwrap();
    let change = AccountChange {
        previous: UserName::parse("agent").unwrap(),
        new: UserName::parse("khing").unwrap(),
    };
    for (root, proof) in [
        (state.to_str().unwrap(), "neither unit config"),
        ("/etc", "protected system"),
    ] {
        std::fs::write(&config, format!("[defaults]\nartifact_root = \"{root}\"\n")).unwrap();
        let error = server_managed_paths(&repo, &config)
            .and_then(|managed| repair_or_refuse(&repo, &change, &managed, true))
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains(proof), "{message}");
        assert!(
            message.contains(Path::new(root).canonicalize().unwrap().to_str().unwrap()),
            "{message}"
        );
        assert!(!bin.path().join("sudo-argv.log").exists());
    }
}

#[test]
fn ownership_unknown_or_root_systemctl_user_refuses_before_mutation() {
    let _exec = crate::test_support::ExecGuard::new();
    for output in ["", " \t\n", "root\n", "garbage:uid\n"] {
        let dir = ownership_tempdir();
        let (repo, config, _) = repair_fixture(dir.path());
        let template = repo.join("packaging/systemd/gah-server.service");
        let body = std::fs::read_to_string(&template).unwrap();
        std::fs::write(&template, body.replace("User=@USER@", "User=nextuser")).unwrap();
        let units = dir.path().join("units");
        std::fs::create_dir(&units).unwrap();
        let target = units.join("gah-server.service");
        let installed = "[Service]\nUser=agent\n";
        std::fs::write(&target, installed).unwrap();
        let bin = unit_install_shims(&target);
        let response = bin.path().join("user-output");
        std::fs::write(&response, output).unwrap();
        let systemctl = bin.path().join("systemctl");
        let shim = std::fs::read_to_string(&systemctl).unwrap();
        let shim = shim
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("User)") {
                    format!("    User) cat '{}' ;;", response.display())
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&systemctl, shim).unwrap();
        let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
        let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));
        let error = super::super::install_server_unit_template_into(
            &repo,
            "gah-server.service",
            &units,
            true,
        )
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("gah-server.service"), "{message}");
        if output.trim() == "root" {
            assert!(
                message.contains("root") && message.contains("nextuser"),
                "{message}"
            );
        } else {
            assert!(
                message.contains("systemctl show -p User --value gah-server.service"),
                "{message}"
            );
        }
        assert!(!bin.path().join("sudo-argv.log").exists(), "{output:?}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), installed);
    }
}

#[test]
fn ownership_persistent_user_drop_in_refuses_before_any_chown() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, _) = repair_fixture(dir.path());
    let units = dir.path().join("units");
    std::fs::create_dir_all(units.join("gah-server.service.d")).unwrap();
    let target = units.join("gah-server.service");
    std::fs::write(&target, format!("[Service]\nUser={}\n", current_user())).unwrap();
    let drop_in = units.join("gah-server.service.d/override.conf");
    std::fs::write(&drop_in, "[Service]\nUser=agent\n").unwrap();
    let bin = unit_install_shims(&target);
    let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
    let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));
    let error =
        super::super::install_server_unit_template_into(&repo, "gah-server.service", &units, true)
            .unwrap_err();
    assert!(
        error.to_string().contains(drop_in.to_str().unwrap()),
        "{error:#}"
    );
    assert!(!bin.path().join("sudo-argv.log").exists());
    let queries = std::fs::read_to_string(bin.path().join("systemctl.log")).unwrap();
    assert!(queries.contains("show -p User --value gah-server.service"));
    assert!(queries.contains("show -p DropInPaths --value gah-server.service"));
}

#[test]
fn ownership_continued_agent_drop_in_halts_at_preflight_before_khing_repair() {
    let _exec = crate::test_support::ExecGuard::new();
    for (body, discovery) in [
        ("[Service]\nUser\\\n=agent\n", "disk"),
        ("[Service]\nUser=agent\n", "disk"),
        ("[Service]\nUser\\\n# comment\n =agent\n", "reported"),
        ("[Service]\\\n\nUser=agent\n", "reported"),
        ("[Service]\nDescription=escaped\\\\\nUser=agent\n", "disk"),
        ("[Service]\nDescription=space\\ \nUser=agent\n", "reported"),
        ("[Service]\nUser=agent\nUser=khing\n", "disk"),
    ] {
        let dir = ownership_tempdir();
        let (repo, config, _) = repair_fixture(dir.path());
        let template = repo.join("packaging/systemd/gah-server.service");
        let rendered = std::fs::read_to_string(&template)
            .unwrap()
            .replace("User=@USER@", "User=khing");
        std::fs::write(&template, rendered).unwrap();
        let units = dir.path().join("units");
        let target = units.join("gah-server.service");
        std::fs::create_dir_all(&units).unwrap();
        let installed = "[Service]\nUser=agent\n";
        std::fs::write(&target, installed).unwrap();
        let drop_in = if discovery == "disk" {
            units.join("gah-server.service.d/override.conf")
        } else {
            dir.path().join("reported/override.conf")
        };
        std::fs::create_dir_all(drop_in.parent().unwrap()).unwrap();
        std::fs::write(&drop_in, body).unwrap();
        let bin = unit_install_shims(&target);
        let systemctl = bin.path().join("systemctl");
        let shim = std::fs::read_to_string(&systemctl).unwrap();
        let shim = shim
            .lines()
            .map(|line| {
                if line.trim_start().starts_with("DropInPaths)") {
                    if discovery == "reported" {
                        format!("    DropInPaths) echo '{}' ;;", drop_in.display())
                    } else {
                        "    DropInPaths) ;;".into()
                    }
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&systemctl, shim).unwrap();
        let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
        let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));
        let error = super::super::install_server_unit_template_into(
            &repo,
            "gah-server.service",
            &units,
            true,
        )
        .unwrap_err();
        let message = format!("{error:#}");
        for expected in [drop_in.to_str().unwrap(), "User=agent", "User=khing"] {
            assert!(message.contains(expected), "{body:?}: {message}");
        }
        let steps = std::fs::read_to_string(bin.path().join("steps.log")).unwrap();
        assert!(steps.contains("show -p DropInPaths"), "{steps}");
        assert!(
            steps.lines().all(|step| step.starts_with("preflight: ")),
            "sequence must halt at preflight: {steps}"
        );
        assert!(!bin.path().join("sudo-argv.log").exists());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), installed);
        assert_eq!(std::fs::read_to_string(&drop_in).unwrap(), body);
    }
}

#[test]
fn ownership_preflight_refuses_source_and_release_before_mutation() {
    use std::os::unix::fs::PermissionsExt;
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, _) = repair_fixture(dir.path());
    let template = repo.join("packaging/systemd/gah-server.service");
    let body = std::fs::read_to_string(&template).unwrap();
    std::fs::write(&template, body.replace("User=@USER@", "User=khing")).unwrap();
    let target = dir.path().join("installed.service");
    std::fs::write(&target, "[Service]\nUser=agent\n").unwrap();
    let bin = unit_install_shims(&target);
    let git = bin.path().join("git");
    let real_git = crate::runner::resolve::resolve_executable_on_path("git").unwrap();
    std::fs::write(
        &git,
        format!(
            r#"#!/bin/sh
if [ "$PWD" != '{repo}' ] && {{ [ "$1" != -C ] || [ "$2" != '{repo}' ]; }}; then exec '{real_git}' "$@"; fi
printf '%s\n' "$*" >> '{log}'
if [ "$3" = rev-parse ] && [ "$4" = --show-toplevel ]; then printf '%s\n' '{repo}'; exit 0; fi
if [ "$1" = remote ] && [ "$2" = get-url ] && [ "$3" = origin ]; then echo https://github.com/Kh1ng/git-agent-harness.git; exit 0; fi
exit 91
"#,
            log = bin.path().join("git.log").display(),
            repo = repo.display(),
            real_git = real_git.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
    let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));
    let before = std::fs::read(repo.join("apps/server/dist/bin.js")).unwrap();
    struct RestoreReexec(Option<std::ffi::OsString>);
    impl Drop for RestoreReexec {
        fn drop(&mut self) {
            match self.0.take() {
                Some(value) => std::env::set_var(super::super::reexec::ENV, value),
                None => std::env::remove_var(super::super::reexec::ENV),
            }
        }
    }
    let _reexec = RestoreReexec(std::env::var_os(super::super::reexec::ENV));
    for repair_ownership in [false, true] {
        let drop_in = target
            .with_file_name("installed.service.d")
            .join("override.conf");
        if repair_ownership {
            std::fs::create_dir_all(drop_in.parent().unwrap()).unwrap();
            std::fs::write(&drop_in, "[Service]\nUser\\\n=agent\n").unwrap();
        }
        let expected = if repair_ownership {
            drop_in.to_str().unwrap()
        } else {
            "--no-repair-ownership"
        };
        for (from_release, resumed) in [(false, false), (true, false), (false, true)] {
            if resumed {
                std::env::set_var(super::super::reexec::ENV, "1");
            } else {
                std::env::remove_var(super::super::reexec::ENV);
            }
            let error = super::super::run(super::super::UpdateArgs {
                repo: Some(repo.clone()),
                pull: true,
                agents: vec![],
                yes: true,
                role: super::super::HostRole::Central,
                restart_server: false,
                server_service: "gah-server.service".into(),
                from_release,
                release_manifest: Some("must-not-read-manifest".into()),
                repair_ownership,
            })
            .unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
        }
        // The release entry point must enforce the gate when called directly too.
        let error = super::super::release::run_release(super::super::release::ReleaseArgs {
            repo: repo.clone(),
            role: super::super::HostRole::Central,
            restart_server: false,
            server_service: "gah-server.service".into(),
            manifest: Some("must-not-read-manifest".into()),
            repair_ownership,
        })
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error:#}");
    }
    assert!(!bin.path().join("sudo-argv.log").exists());
    assert_eq!(
        std::fs::read(repo.join("apps/server/dist/bin.js")).unwrap(),
        before
    );
    let commands = std::fs::read_to_string(bin.path().join("git.log")).unwrap();
    assert_eq!(
        commands
            .lines()
            .filter(|line| line.ends_with("rev-parse --show-toplevel"))
            .count(),
        6
    );
    assert!(
        commands
            .lines()
            .all(|line| line.ends_with("rev-parse --show-toplevel")
                || line == "worktree list --porcelain"
                || line == "remote get-url origin"),
        "no mutating git command may run: {commands}"
    );
}

#[test]
fn ownership_protected_home_and_mount_roots_are_never_targets() {
    let dir = ownership_tempdir();
    let child = dir.path().join("data");
    std::fs::create_dir(&child).unwrap();
    let anchor = dir.path().canonicalize().unwrap();
    assert_eq!(
        confirmed_chown_target(dir.path(), std::slice::from_ref(&anchor)),
        None
    );
    assert_eq!(
        confirmed_chown_target(&child, &[anchor]),
        Some(child.canonicalize().unwrap())
    );
    for root in ["/", "/home", "/Users", "/root", "/dev", "/proc", "/sys"] {
        assert_eq!(
            confirmed_chown_target(Path::new(root), &[PathBuf::from("/")]),
            None,
            "{root}"
        );
    }
    if let Some(home) = std::env::var_os("HOME") {
        assert_eq!(
            confirmed_chown_target(Path::new(&home), &[PathBuf::from("/")]),
            None
        );
    }
}

#[test]
fn ownership_first_install_also_refuses_persistent_user_drop_in() {
    let _exec = crate::test_support::ExecGuard::new();
    let dir = ownership_tempdir();
    let (repo, config, _) = repair_fixture(dir.path());
    let units = dir.path().join("units");
    let drop_in = units.join("gah-server.service.d/override.conf");
    std::fs::create_dir_all(drop_in.parent().unwrap()).unwrap();
    std::fs::write(&drop_in, "[Service]\nUser=agent\n").unwrap();
    let bin = unit_install_shims(&units.join("gah-server.service"));
    let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
    let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));
    let error =
        super::super::install_server_unit_template_into(&repo, "gah-server.service", &units, true)
            .unwrap_err();
    assert!(
        error.to_string().contains(drop_in.to_str().unwrap()),
        "{error:#}"
    );
    assert!(!bin.path().join("sudo-argv.log").exists());
}

#[cfg(unix)]
#[test]
fn ownership_unwritable_state_boundary_stops_preflight() {
    let _exec = crate::test_support::ExecGuard::new();
    use std::os::unix::fs::PermissionsExt;
    let dir = ownership_tempdir();
    let (repo, config, state) = repair_fixture(dir.path());
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o555)).unwrap();
    if unsafe { libc::geteuid() } != 0 {
        let error = server_managed_paths(&repo, &config).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("repair this boundary's permissions manually"),
            "{error:#}"
        );
    }
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o755)).unwrap();
}
