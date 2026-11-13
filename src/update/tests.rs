use super::{
    copy_opencode_agent_configs, ensure_clean, ensure_default_branch_checkout,
    ensure_lockfile_current, install_prune_unit_template, install_quota_refresh_unit_template,
    install_watchdog_unit_template, installed_binary_path, resolve_web_deploy_root, run,
    stale_asset_names, try_enable_linger, HostRole, UpdateArgs, WEB_BUILD_ARGS,
};
use crate::test_support::{ExecGuard, PathGuard};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use tempfile::TempDir;

static XDG_CONFIG_HOME_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn claude_only_installs_no_opencode_files_or_quota_units() {
    let config = TempDir::new().unwrap();
    let agents = vec!["claude".to_string()];
    super::install_selected_agent_assets(
        Path::new(env!("CARGO_MANIFEST_DIR")),
        config.path(),
        &agents,
    )
    .unwrap();
    assert!(!config.path().join("opencode").exists());
    assert!(!config.path().join("systemd").exists());
    let plan = super::installation_plan(HostRole::Worker, &agents).unwrap();
    assert!(!plan
        .iter()
        .any(|line| line.contains("OpenCode") || line.contains("quota-refresh")));
    assert!(super::quota_refresh_selected(&["codex".into()]));
    assert!(super::quota_refresh_selected(&["vibe".into()]));
}

#[test]
fn a_stale_lockfile_stops_the_update_before_install() {
    // A path dependency keeps this offline: no registry lookup is needed.
    let tmp = TempDir::new().unwrap();
    for (name, deps) in [("dep", ""), ("app", "dep = { path = \"../dep\" }")] {
        let dir = tmp.path().join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "").unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n\n[dependencies]\n{deps}"),
        )
        .unwrap();
    }
    let app = tmp.path().join("app");
    let lock = |args: &[&str]| {
        let status = Command::new("cargo")
            .args(args)
            .current_dir(&app)
            .status()
            .unwrap();
        assert!(status.success());
    };
    lock(&["generate-lockfile", "--offline"]);
    ensure_lockfile_current(&app).unwrap();

    let manifest = app.join("Cargo.toml");
    let without_dep = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("dep = { path = \"../dep\" }", "");
    std::fs::write(&manifest, without_dep).unwrap();
    lock(&["generate-lockfile", "--offline"]);
    std::fs::write(
        &manifest,
        std::fs::read_to_string(&manifest).unwrap() + "dep = { path = \"../dep\" }\n",
    )
    .unwrap();

    let error = ensure_lockfile_current(&app).unwrap_err().to_string();
    assert!(error.contains("does not match Cargo.toml"), "{error}");
}

/// Scoped override for `XDG_CONFIG_HOME`, mirroring the other process-env
/// guards in `crate::test_support` -- must be restored before another
/// parallel test can observe process state.
struct XdgConfigHomeGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    original: Option<OsString>,
}

impl XdgConfigHomeGuard {
    fn set(path: impl AsRef<std::ffi::OsStr>) -> Self {
        let lock = XDG_CONFIG_HOME_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let original = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", path);
        Self {
            _lock: lock,
            original,
        }
    }
}

impl Drop for XdgConfigHomeGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository_with_origin_head() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("README.md"), "test\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "initial"]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(
        &repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    (tmp, repo)
}

#[test]
fn installed_binary_lives_in_cargo_bin() {
    let path = installed_binary_path().unwrap();
    assert_eq!(path.file_name().and_then(|name| name.to_str()), Some("gah"));
    assert_eq!(
        path.parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str()),
        Some("bin")
    );
}

#[test]
fn update_assets_keep_opencode_roles_distinct() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config_home = TempDir::new().unwrap();

    let [reviewer, implementer] = copy_opencode_agent_configs(repo, config_home.path()).unwrap();

    let reviewer = std::fs::read_to_string(reviewer).unwrap();
    let implementer = std::fs::read_to_string(implementer).unwrap();
    assert!(reviewer.contains("bash: false"));
    assert!(reviewer.contains("read: false"));
    assert!(implementer.contains("bash: true"));
    assert!(implementer.contains("read: true"));
}

#[test]
fn default_branch_check_accepts_origin_default_and_rejects_feature_branch() {
    let (_tmp, repo) = repository_with_origin_head();
    ensure_default_branch_checkout(&repo).unwrap();
    git(&repo, &["checkout", "-b", "feature"]);
    let err = ensure_default_branch_checkout(&repo).unwrap_err();
    assert!(err.to_string().contains("non-default branch 'feature'"));
}

#[test]
fn default_branch_check_explains_missing_origin_head() {
    let (_tmp, repo) = repository_with_origin_head();
    git(&repo, &["symbolic-ref", "-d", "refs/remotes/origin/HEAD"]);
    let err = ensure_default_branch_checkout(&repo).unwrap_err();
    assert!(err.to_string().contains("git remote set-head origin -a"));
}

#[test]
fn clean_check_rejects_uncommitted_changes() {
    let (_tmp, repo) = repository_with_origin_head();
    ensure_clean(&repo).unwrap();
    std::fs::write(repo.join("dirty.txt"), "dirty\n").unwrap();
    let err = ensure_clean(&repo).unwrap_err();
    assert!(err.to_string().contains("dirty.txt"));
}

/// Issue #726 AC1/AC7: `gah update` installs the watchdog unit
/// deterministically and reloads the user systemd manager, but never
/// enables or starts anything -- only `daemon-reload` may appear in the
/// recorded systemctl invocations.
#[test]
fn watchdog_unit_template_is_installed_without_enabling_or_starting_anything() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config_tmp = TempDir::new().unwrap();
    let _xdg_guard = XdgConfigHomeGuard::set(config_tmp.path());

    let bin_tmp = TempDir::new().unwrap();
    let record_path = bin_tmp.path().join("argv.log");
    let script = format!("#!/bin/sh\necho \"$@\" >> '{}'\n", record_path.display());
    let script_path = bin_tmp.path().join("systemctl");
    std::fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }
    let _path_guard = PathGuard::set(bin_tmp.path().to_str().unwrap());

    let [service, timer] = install_watchdog_unit_template(repo).unwrap().unwrap();

    assert!(service.ends_with("systemd/user/gah-watchdog.service"));
    assert!(timer.ends_with("systemd/user/gah-watchdog.timer"));
    assert!(
        std::fs::read_to_string(&service)
            .unwrap()
            .contains("gah watchdog-check"),
        "installed unit should invoke the packaged watchdog-check command"
    );
    assert!(
        !std::fs::read_to_string(&service)
            .unwrap()
            .contains("/home/khing/workspace/agent-lab"),
        "installed unit must not reference the old host-local script path"
    );

    let record = std::fs::read_to_string(&record_path).unwrap();
    assert!(!record.is_empty(), "expected systemctl to be invoked");
    for forbidden in ["start", "restart", "enable"] {
        assert!(
            !record.split_whitespace().any(|arg| arg == forbidden),
            "systemctl was invoked with forbidden verb '{forbidden}': {record}"
        );
    }
    assert!(record.contains("daemon-reload"), "{record}");
}

/// Issue #761: `gah update` installs BOTH quota-refresh units and
/// enables the timer (unlike the opt-in watchdog), so `quota_observations.jsonl`
/// gets populated automatically on deployed nodes. The fake systemctl
/// logs every invocation and succeeds; the assertion checks both files
/// landed and the timer was enabled.
#[test]
fn quota_refresh_units_are_installed_and_timer_enabled() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config_tmp = TempDir::new().unwrap();
    let _xdg_guard = XdgConfigHomeGuard::set(config_tmp.path());

    let bin_tmp = TempDir::new().unwrap();
    let record_path = bin_tmp.path().join("argv.log");
    let script = format!("#!/bin/sh\necho \"$@\" >> '{}'\n", record_path.display());
    let script_path = bin_tmp.path().join("systemctl");
    std::fs::write(&script_path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }
    let _path_guard = PathGuard::set(bin_tmp.path().to_str().unwrap());

    let [service, timer] = install_quota_refresh_unit_template(repo).unwrap().unwrap();

    assert!(service.ends_with("systemd/user/gah-quota-refresh.service"));
    assert!(timer.ends_with("systemd/user/gah-quota-refresh.timer"));
    assert!(
        std::fs::read_to_string(&service)
            .unwrap()
            .contains("gah quota auto-refresh"),
        "installed unit should invoke the packaged auto-refresh command"
    );
    assert!(
        std::fs::read_to_string(&timer)
            .unwrap()
            .contains("gah-quota-refresh.service"),
        "timer must point at the packaged service"
    );

    let record = std::fs::read_to_string(&record_path).unwrap();
    assert!(record.contains("daemon-reload"), "{record}");
    assert!(
        record.contains("enable") && record.contains("gah-quota-refresh.timer"),
        "timer must be enabled: {record}"
    );
}

#[test]
fn prune_units_keep_chat_maintenance_on_the_existing_daily_timer() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config_tmp = TempDir::new().unwrap();
    let _xdg_guard = XdgConfigHomeGuard::set(config_tmp.path());
    let bin_tmp = TempDir::new().unwrap();
    let record_path = bin_tmp.path().join("argv.log");
    let script = format!("#!/bin/sh\necho \"$@\" >> '{}'\n", record_path.display());
    let systemctl = bin_tmp.path().join("systemctl");
    std::fs::write(&systemctl, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&systemctl).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&systemctl, perms).unwrap();
    }
    let _path_guard = PathGuard::set(bin_tmp.path().to_str().unwrap());

    let [service, timer] = install_prune_unit_template(repo).unwrap().unwrap();
    let service_text = std::fs::read_to_string(service).unwrap();
    assert!(service_text.contains("gah prune"));
    assert!(service_text.contains("chatMaintenanceCli.js"));
    assert!(std::fs::read_to_string(timer)
        .unwrap()
        .contains("OnCalendar="));
    let record = std::fs::read_to_string(record_path).unwrap();
    assert!(record.contains("enable --now gah-prune.timer"), "{record}");
}

#[test]
fn host_role_parses_only_known_values() {
    assert_eq!(HostRole::parse("central").unwrap(), HostRole::Central);
    assert_eq!(HostRole::parse("worker").unwrap(), HostRole::Worker);
    assert!(HostRole::parse("bogus").is_err());
}

/// A worker node never runs `gah-server.service` -- `--restart-server`
/// on `--role worker` is a contradiction, not something to silently
/// ignore or default away.
#[test]
fn worker_role_rejects_restart_server_flag() {
    let err = run(UpdateArgs {
        repo: None,
        pull: false,
        agents: vec![],
        yes: true,
        role: HostRole::Worker,
        restart_server: true,
        server_service: "gah-server.service".into(),
        from_release: false,
        release_manifest: None,
        repair_ownership: true,
    })
    .unwrap_err();
    assert!(err.to_string().contains("--role central"));
}

/// Issue #894: `gah update` (central role) reinstalls the system-level
/// `gah-server.service` unit from the tracked template on every run.
/// Moved to `src/update/ownership.rs` with the #1626 repair tests; the
/// coverage lives next to the install seam they share.

#[test]
fn empty_web_deploy_root_skips_deployment() {
    assert_eq!(
        resolve_web_deploy_root(Some(OsString::new())).unwrap(),
        None
    );
    assert!(resolve_web_deploy_root(Some(OsString::from("relative"))).is_err());
    assert!(resolve_web_deploy_root(Some(OsString::from("/"))).is_err());
    assert!(resolve_web_deploy_root(Some(OsString::from("/tmp/.."))).is_err());
    assert!(resolve_web_deploy_root(Some(OsString::from("/var/www/gah/../../.."))).is_err());
    // Unset copies only into a web root an earlier install created (#1327).
    let legacy = Path::new("/var/www/gah");
    assert_eq!(
        resolve_web_deploy_root(None).unwrap(),
        legacy.is_dir().then(|| legacy.to_path_buf())
    );
}

/// Issue #1010: the production updater must build `apps/web` directly,
/// not via the root `build:web` script -- that script's `prebuild:web`
/// lifecycle hook reruns the whole server integration suite, which can
/// hang and wedge `gah update` on the very host it's supposed to update.
#[test]
fn web_build_targets_workspace_directly_not_root_build_web_script() {
    assert_eq!(WEB_BUILD_ARGS, &["run", "--workspace=apps/web", "build"]);
    assert!(!WEB_BUILD_ARGS.contains(&"build:web"));
}

#[cfg(unix)]
#[test]
fn web_deploy_makes_restrictive_build_assets_readable_to_the_service() {
    use std::os::unix::fs::PermissionsExt;
    let _exec_guard = ExecGuard::new();
    let tmp = TempDir::new().unwrap();
    let dist = tmp.path().join("build output");
    let root = tmp.path().join("public web");
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(dist.join("assets/nested")).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["index.html", "assets/app.js", "assets/nested/app.css"] {
        let path = dist.join(name);
        std::fs::write(&path, name).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    std::fs::set_permissions(
        dist.join("assets/nested"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let operator_file = root.join("robots.txt");
    std::fs::write(&operator_file, "operator content").unwrap();
    std::fs::set_permissions(&operator_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    // Execute real copy/install/chmod under a restrictive subprocess umask.
    // Only privileged directory ownership is simulated inside this fixture.
    shim(
        &bin,
        "sudo",
        r#"umask 077
if [ "$1" = install ] && [ "$2" = -d ]; then
  for last in "$@"; do :; done
  exec /usr/bin/install -d -m 0755 "$last"
fi
exec "$@"
"#,
    );
    let _path = PathGuard::set(format!("{}:/usr/bin:/bin", bin.display()));
    super::deploy_web_dist(tmp.path(), &dist, root.clone()).unwrap();
    for name in ["index.html", "assets/app.js", "assets/nested/app.css"] {
        let path = root.join(name);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), name);
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o444,
            0o444,
            "{name}"
        );
    }
    assert_eq!(
        std::fs::metadata(root.join("assets/nested"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0o111
    );
    assert_eq!(
        std::fs::metadata(operator_file)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(dist.join("assets/app.js"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
}

/// Issue #896 review: the deploy prunes stale hashed assets. Vite
/// content-hashes filenames, so any file in the deployed assets dir that
/// the new build doesn't reference is stale -- and a new asset that IS
/// referenced must never be pruned.
#[test]
fn prune_stale_web_assets_keeps_new_and_drops_old_hashed_files() {
    let keep: HashSet<String> = ["index-new-def.js", "index-new-ghi.css"]
        .into_iter()
        .map(String::from)
        .collect();
    let listing = "/var/www/gah/assets/index-old-abc.js\n\
                        /var/www/gah/assets/index-new-def.js\n\
                        /var/www/gah/assets/index-new-ghi.css\n";
    assert_eq!(
        stale_asset_names(listing, &keep),
        vec!["index-old-abc.js".to_string()]
    );
    // An empty listing (no assets deployed yet) prunes nothing.
    assert!(stale_asset_names("", &keep).is_empty());
}

/// Executable fake for the user-lingering tests: same shim shape as the
/// hand-rolled sudo/systemctl fakes above.
fn shim(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
    }
}

/// Fakes loginctl and sudo, each exiting with the given code and logging
/// its argv so a test can see what ran.
fn linger_shims(loginctl_exit: u8, sudo_exit: u8) -> TempDir {
    let bin = TempDir::new().unwrap();
    for (name, exit) in [("loginctl", loginctl_exit), ("sudo", sudo_exit)] {
        let log = bin.path().join(format!("{name}.log"));
        shim(
            bin.path(),
            name,
            &format!(
                "printf '%s\\n' \"$*\" >> '{}'\nexit {exit}\n",
                log.display()
            ),
        );
    }
    bin
}

/// Issue #1347: polkit first, then sudo; only central may prompt.
#[test]
fn user_lingering_tries_polkit_then_passwordless_sudo() {
    let _exec_guard = ExecGuard::new();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let log = |bin: &TempDir, name: &str| {
        std::fs::read_to_string(bin.path().join(format!("{name}.log")))
            .map(|log| log.trim().to_string())
            .ok()
    };

    let bin = linger_shims(0, 0);
    let _path_guard = PathGuard::set(bin.path().to_str().unwrap());
    assert!(try_enable_linger(repo, "testuser", false));
    assert_eq!(
        log(&bin, "loginctl").as_deref(),
        Some("--no-ask-password enable-linger")
    );
    assert_eq!(log(&bin, "sudo"), None, "polkit allowed it: no sudo");
    drop(_path_guard);

    let bin = linger_shims(1, 0);
    let _path_guard = PathGuard::set(bin.path().to_str().unwrap());
    assert!(try_enable_linger(repo, "testuser", false));
    assert_eq!(
        log(&bin, "sudo").as_deref(),
        Some("-n loginctl enable-linger testuser")
    );
    drop(_path_guard);

    let bin = linger_shims(1, 0);
    let _path_guard = PathGuard::set(bin.path().to_str().unwrap());
    assert!(try_enable_linger(repo, "testuser", true));
    assert_eq!(
        log(&bin, "sudo").as_deref(),
        Some("loginctl enable-linger testuser"),
        "central may prompt for the sudo password"
    );
    drop(_path_guard);

    let bin = linger_shims(1, 1);
    let _path_guard = PathGuard::set(bin.path().to_str().unwrap());
    assert!(!try_enable_linger(repo, "testuser", false));
}
