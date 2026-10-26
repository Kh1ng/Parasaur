//! Repair ownership of the server-managed paths when `gah update` changes the
//! service account (issue #1626).
//!
//! #1330 renders `gah-server.service` for the installing account. When the
//! previously installed unit ran as a different account, that account owns
//! the state and config the server reads and writes, so installing the new
//! unit without repairing ownership crash-loops the server (`EACCES` on the
//! registry config, then `EPERM` chmod'ing the push keys). The updater repairs
//! by default (`sudo chown -R`, printed like every other privileged step) or,
//! with `--no-repair-ownership`, refuses before replacing the installed unit.
//!
//! Never chown outside the server-managed paths, and never follow a symlink
//! out of them: a managed path that is a symlink is only chowned after its
//! resolved target is confirmed to be under the checkout (the install root)
//! or one of the configured state roots.

use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The service account of a previously installed unit, as far as the updater
/// can determine it from the installed unit file.
#[derive(Debug)]
pub(super) enum PreviousAccount {
    /// No unit is installed at the target yet: a first install, with no
    /// account change to repair.
    NotInstalled,
    /// The unit exists but carries no `User=`: systemd runs it as root, so
    /// its files are root-owned and a new account needs the repair.
    Root,
    User(String),
}

/// The `User=` account named by an installed or rendered unit. systemd keys
/// are case-sensitive and the render pipeline writes exactly one `User=`
/// line, so the parse looks for that line and nothing else.
pub(super) fn unit_user(unit: &str) -> Option<String> {
    unit.lines()
        .map(str::trim_start)
        .filter_map(|line| line.strip_prefix("User="))
        .map(str::trim)
        .next_back()
        .map(str::to_string)
}

/// Read the account the installed unit runs as. A missing unit is a first
/// install; a unit that exists but cannot be read is announced and treated
/// the same way (the swap below still replaces it, this only skips the
/// comparison).
pub(super) fn previous_account(unit_path: &Path) -> PreviousAccount {
    if !unit_path.exists() {
        return PreviousAccount::NotInstalled;
    }
    let Ok(unit) = std::fs::read_to_string(unit_path) else {
        eprintln!(
            "[gah update] cannot read {} to compare its User=; skipping the ownership repair check",
            unit_path.display()
        );
        return PreviousAccount::NotInstalled;
    };
    match unit_user(&unit) {
        Some(user) if user != "root" => PreviousAccount::User(user),
        // An explicit `User=root` and a missing `User=` are the same account.
        Some(_) | None => PreviousAccount::Root,
    }
}

/// The service-account change that requires an ownership repair: the new
/// account differs from the previous one. `None` when there is no previously
/// installed unit, or the accounts already match.
pub(super) struct AccountChange {
    pub(super) previous: String,
    pub(super) new: String,
}

pub(super) fn account_change(
    previous: PreviousAccount,
    new_user: Option<String>,
) -> Option<AccountChange> {
    let previous = match previous {
        PreviousAccount::NotInstalled => return None,
        PreviousAccount::Root => "root".to_string(),
        PreviousAccount::User(user) => user,
    };
    let new = new_user?;
    (previous != new).then_some(AccountChange { previous, new })
}

/// The server-managed paths the service account must be able to read and
/// write, plus the roots those paths may resolve into.
#[derive(Debug)]
pub(super) struct ServerManagedPaths {
    /// Configured paths in print order: the state roots declared by the
    /// config the unit points at, then the checkout's `config/` (registry
    /// config, push keys), `apps/server/dist`, and the web deploy root when
    /// one is configured.
    pub(super) paths: Vec<PathBuf>,
    /// Canonical roots a managed path may resolve into without escaping:
    /// the checkout, the state roots, and the web deploy root.
    pub(super) anchors: Vec<PathBuf>,
}

/// Collect the server-managed paths for `repo`, reading state roots from the
/// same config the rendered unit's `GAH_CONFIG_PATH` points at. A config
/// that exists but cannot be parsed is a hard error: the repair must not
/// guess at the state root. A missing config contributes no state roots.
pub(super) fn server_managed_paths(repo: &Path, config_path: &Path) -> Result<ServerManagedPaths> {
    let mut state_roots: BTreeSet<PathBuf> = BTreeSet::new();
    if config_path.exists() {
        let config_text = config_path
            .to_str()
            .context("the server config path is not UTF-8")?;
        let config = crate::config::load(Some(config_text)).with_context(|| {
            format!(
                "reading {} to locate the state roots for the ownership repair",
                config_path.display()
            )
        })?;
        for root in std::iter::once(&config.defaults.artifact_root).chain(
            config
                .profiles
                .values()
                .map(|profile| &profile.artifact_root),
        ) {
            let root = root.trim();
            if !root.is_empty() {
                state_roots.insert(PathBuf::from(root));
            }
        }
    }

    let mut paths: Vec<PathBuf> = state_roots
        .iter()
        // chown -R on a parent root already repairs a nested one; keeping
        // only the topmost roots keeps the printed commands readable.
        .filter(|root| {
            !state_roots
                .iter()
                .any(|other| other != *root && root.starts_with(other))
        })
        .cloned()
        .collect();
    let web_root = super::resolve_web_deploy_root(std::env::var_os("GAH_WEB_DEPLOY_ROOT"))?;
    for managed in [repo.join("config"), repo.join("apps/server/dist")] {
        if !paths.contains(&managed) {
            paths.push(managed);
        }
    }
    if let Some(web_root) = &web_root {
        if !paths.contains(web_root) {
            paths.push(web_root.clone());
        }
    }
    let anchors = std::iter::once(repo)
        .chain(state_roots.iter().map(PathBuf::as_path))
        .chain(web_root.as_deref())
        .filter_map(|root| std::fs::canonicalize(root).ok())
        .collect();
    Ok(ServerManagedPaths { paths, anchors })
}

/// Resolve a managed path (following any symlink) and confirm the target is
/// a directory equal to or under one of the anchors. `None` when the path is
/// missing or dangling, not a directory, or resolves outside every anchor:
/// the repair must never chown it.
fn confirmed_chown_target(path: &Path, anchors: &[PathBuf]) -> Option<PathBuf> {
    if path.symlink_metadata().is_err() {
        return None;
    }
    let resolved = std::fs::canonicalize(path).ok()?;
    if !resolved.is_dir() {
        return None;
    }
    let inside = |anchor: &Path| resolved == anchor || resolved.starts_with(anchor);
    anchors
        .iter()
        .any(|anchor| inside(anchor))
        .then_some(resolved)
}

/// Repair the server-managed paths for the account change (default), or
/// refuse it. Refusal names both accounts and the paths; repair prints every
/// `sudo chown -R` it runs through the updater's shared privileged-step
/// mechanism and fails the update if any chown fails, so the installed unit
/// is only ever replaced once the new account can run the server.
pub(super) fn repair_or_refuse(
    repo: &Path,
    change: &AccountChange,
    managed: &ServerManagedPaths,
    repair: bool,
) -> Result<()> {
    let listed = managed
        .paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n  ");
    if !repair {
        bail!(
            "the installed gah-server.service runs as User={} but this update renders User={}; \
             --no-repair-ownership was passed, so refusing to change the service account while \
             the new account cannot read and write the server-managed paths, which would \
             crash-loop the server (issue #1626):\n  {listed}\nRerun without \
             --no-repair-ownership to repair them with sudo chown -R, or chown them to \
             User={} yourself first.",
            change.previous,
            change.new,
            change.new
        );
    }
    println!(
        "gah-server.service changes User={} to User={}; repairing ownership of the \
         server-managed paths so the new account can run the server (issue #1626).",
        change.previous, change.new
    );
    let mut repaired = 0usize;
    for path in &managed.paths {
        let Some(target) = confirmed_chown_target(path, &managed.anchors) else {
            if path.exists() {
                eprintln!(
                    "[gah update] skipped ownership repair for {}: it is not a directory, or it \
                     resolves outside the checkout and the state roots. Inspect it, then run \
                     `sudo chown -R -- {} <path>` yourself if it should belong to the new account.",
                    path.display(),
                    change.new
                );
            }
            continue;
        };
        let target = target
            .to_str()
            .context("the chown target path is not UTF-8")?;
        super::run_command(repo, "sudo", &["chown", "-R", "--", &change.new, target])?;
        repaired += 1;
    }
    println!(
        "Repaired ownership of {repaired} server-managed path(s) for User={}. The loop, \
         watchdog, prune and quota-refresh user units install for '{}' and print their \
         locations below.",
        change.new, change.new
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        account_change, confirmed_chown_target, previous_account, repair_or_refuse,
        server_managed_paths, unit_user, AccountChange, PreviousAccount,
    };
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use tempfile::TempDir;

    #[test]
    fn unit_user_reads_the_service_account_line() {
        let unit = "[Unit]\nDescription=Parasaur API\n\n[Service]\nUser=agent\nExecStart=node\n";
        assert_eq!(unit_user(unit).as_deref(), Some("agent"));
        // Whitespace-tolerant, and a missing User= names no account.
        assert_eq!(unit_user("  User = spaced\n").as_deref(), None);
        assert_eq!(unit_user("  User=  spaced  \n").as_deref(), Some("spaced"));
        assert_eq!(unit_user("[Service]\nExecStart=node\n"), None);
        assert_eq!(unit_user("NotUser=x\nUser=root\n").as_deref(), Some("root"));
        // A later User= line wins, matching systemd's last-assignment rule.
        assert_eq!(
            unit_user("User=first\nUser=second\n").as_deref(),
            Some("second")
        );
    }

    #[test]
    fn previous_account_distinguishes_missing_root_and_user_units() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("absent.service");
        assert!(matches!(
            previous_account(&missing),
            PreviousAccount::NotInstalled
        ));
        for (name, unit, expected) in [
            ("root-implicit", "[Service]\n", PreviousAccount::Root),
            (
                "root-explicit",
                "[Service]\nUser=root\n",
                PreviousAccount::Root,
            ),
            (
                "named",
                "[Service]\nUser=agent\n",
                PreviousAccount::User("agent".into()),
            ),
        ] {
            let path = dir.path().join(name);
            std::fs::write(&path, unit).unwrap();
            match (previous_account(&path), expected) {
                (PreviousAccount::Root, PreviousAccount::Root) => {}
                (PreviousAccount::User(found), PreviousAccount::User(expected)) => {
                    assert_eq!(found, expected)
                }
                other => panic!("{name}: {other:?}"),
            }
        }
    }

    #[test]
    fn account_change_requires_an_installed_unit_and_a_different_account() {
        use PreviousAccount::{NotInstalled, Root, User};
        assert!(account_change(NotInstalled, Some("khing".into())).is_none());
        assert!(account_change(User("khing".into()), Some("khing".into())).is_none());
        assert!(account_change(User("khing".into()), None).is_none());
        for (previous, new, expected) in [
            (Root, "khing", "root"),
            (User("agent".into()), "khing", "agent"),
        ] {
            let change = account_change(previous, Some(new.into())).unwrap();
            assert_eq!(change.previous, expected);
            assert_eq!(change.new, new);
        }
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
        let dir = TempDir::new().unwrap();
        let repo = fixture_repo(dir.path());
        let state = dir.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let config = dir.path().join("config.toml");
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
            managed.paths,
            vec![
                PathBuf::from("/srv/gah"),
                state.clone(),
                repo.join("config"),
                repo.join("apps/server/dist"),
                web.clone(),
            ],
            "the nested /srv/gah/a root must be covered by /srv/gah"
        );
        for anchor in [repo.canonicalize().unwrap(), web.canonicalize().unwrap()] {
            assert!(
                managed.anchors.contains(&anchor),
                "missing anchor {}",
                anchor.display()
            );
        }
        drop(env);
        // An explicitly-empty GAH_WEB_DEPLOY_ROOT opts out of the web root.
        let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));
        let managed = server_managed_paths(&repo, &config).unwrap();
        assert!(!managed.paths.contains(&web));
    }

    #[test]
    fn malformed_config_fails_the_repair_rather_than_guessing_state_roots() {
        let dir = TempDir::new().unwrap();
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
        let dir = TempDir::new().unwrap();
        let inside = dir.path().join("inside");
        let outside = dir.path().join("outside");
        for path in [&inside, &outside] {
            std::fs::create_dir_all(path).unwrap();
        }
        let anchors = vec![inside.canonicalize().unwrap()];
        assert_eq!(
            confirmed_chown_target(&inside, &anchors),
            Some(inside.canonicalize().unwrap())
        );
        assert_eq!(confirmed_chown_target(&outside, &anchors), None);
        // A symlink that stays under the anchor resolves to its target; one
        // that escapes is refused, and a dangling or missing path is nothing
        // to repair.
        let link = dir.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&inside, &link).unwrap();
        assert_eq!(
            confirmed_chown_target(&link, &anchors),
            Some(inside.canonicalize().unwrap())
        );
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
        let dir = TempDir::new().unwrap();
        let repo = fixture_repo(dir.path());
        let state = dir.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let web = dir.path().join("www");
        std::fs::create_dir_all(&web).unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(
            &config,
            format!("[defaults]\nartifact_root = \"{}\"\n", state.display()),
        )
        .unwrap();
        let _env = ManagedPathsEnvGuard::set(Some(web.to_str().unwrap()), Some(&config));
        let managed = server_managed_paths(&repo, &config).unwrap();
        let change = AccountChange {
            previous: "agent".into(),
            new: "khing".into(),
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
        let bin = TempDir::new().unwrap();
        let sudo_log = bin.path().join("sudo-argv.log");
        for (name, body) in [
            (
                "sudo",
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = install ]; then cp \"$8\" '{}'; fi\nexit 0\n",
                    sudo_log.display(),
                    unit_target.display()
                )
                .as_str(),
            ),
            (
                "systemctl",
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'systemd 255'; exit 0; fi\nexit 0\n",
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
        std::fs::create_dir_all(&state).unwrap();
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
        let dir = TempDir::new().unwrap();
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

        let target = super::super::install_server_unit_template_into(
            &repo,
            "gah-server.service",
            &units,
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(target, units.join("gah-server.service"));

        let record = std::fs::read_to_string(bin.path().join("sudo-argv.log")).unwrap();
        let user = current_user();
        for managed in [&state, &repo.join("config"), &repo.join("apps/server/dist")] {
            let chown = format!(
                "chown -R -- {user} {}",
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
        let dir = TempDir::new().unwrap();
        let (repo, config, state) = repair_fixture(dir.path());
        let units = dir.path().join("units");
        std::fs::create_dir_all(&units).unwrap();
        std::fs::write(units.join("gah-server.service"), "[Service]\nUser=agent\n").unwrap();
        let bin = unit_install_shims(&units.join("gah-server.service"));
        let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
        let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));

        let error = super::super::install_server_unit_template_into(
            &repo,
            "gah-server.service",
            &units,
            false,
        )
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
        for installed in [Some(current_user()), None] {
            let dir = TempDir::new().unwrap();
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
            let bin = unit_install_shims(&units.join("gah-server.service"));
            let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
            let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));

            super::super::install_server_unit_template_into(
                &repo,
                "gah-server.service",
                &units,
                true,
            )
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
        let dir = TempDir::new().unwrap();
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
                "chown -R -- {} {}",
                current_user(),
                state.canonicalize().unwrap().display()
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
        let dir = TempDir::new().unwrap();
        let (repo, config, _state) = repair_fixture(dir.path());
        let units = dir.path().join("units");
        std::fs::create_dir_all(&units).unwrap();
        let bin = unit_install_shims(&units.join("gah-server.service"));
        let _path = crate::test_support::PathGuard::set(bin.path().to_str().unwrap());
        let _env = ManagedPathsEnvGuard::set(Some(""), Some(&config));

        let target = super::super::install_server_unit_template_into(
            &repo,
            "gah-server.service",
            &units,
            true,
        )
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
        let dir = TempDir::new().unwrap();
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
        let dir = TempDir::new().unwrap();
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
}
