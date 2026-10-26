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
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A validated system account name, safe to include in diagnostics and argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UserName(String);

impl UserName {
    fn parse(value: &str) -> Result<Self> {
        let bytes = value.as_bytes();
        if !(1..=32).contains(&bytes.len())
            || !matches!(bytes[0], b'a'..=b'z' | b'_')
            || !bytes
                .iter()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
        {
            bail!("unsupported systemd service account name");
        }
        Ok(Self(value.to_owned()))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for UserName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug)]
pub(super) enum PreviousAccount {
    NotInstalled,
    Root,
    User(UserName),
}

/// Read the last User assignment in the Service section. An empty assignment
/// resets systemd to root; directives in other sections have no effect. This
/// fallback also reads files not yet reflected in systemd's cached properties.
pub(super) fn unit_user(unit: &str) -> Option<String> {
    let unit = unit
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut logical = String::new();
    for line in unit.lines() {
        if line.trim_start_matches([' ', '\t']).starts_with(['#', ';']) {
            continue;
        }
        let trailing_slashes = line.bytes().rev().take_while(|byte| *byte == b'\\').count();
        if trailing_slashes % 2 == 1 {
            logical.push_str(&line[..line.len() - 1]);
            logical.push(' ');
        } else {
            logical.push_str(line);
            logical.push('\n');
        }
    }
    let mut service = false;
    let mut user = None;
    for line in logical.lines().map(|line| line.trim_matches([' ', '\t'])) {
        if line.starts_with('[') {
            service = line == "[Service]";
        } else if service {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim_matches([' ', '\t']) == "User" {
                    // User= is a raw string, not a shell word: quotes and escapes
                    // remain part of the account, as reported by systemctl show.
                    user = Some(value.trim_matches([' ', '\t']).to_string());
                }
            }
        }
    }
    user
}

fn account(value: &str) -> Result<PreviousAccount> {
    let user = UserName::parse(value)?;
    Ok(if user.as_str() == "root" {
        PreviousAccount::Root
    } else {
        PreviousAccount::User(user)
    })
}

/// Only ENOENT means first install. Permission and all other read failures
/// must stop the update before it changes any installed artifacts.
pub(super) fn previous_account(unit_path: &Path) -> Result<PreviousAccount> {
    match std::fs::read_to_string(unit_path) {
        Ok(unit) => {
            let value = unit_user(&unit).unwrap_or_default();
            account(if value.is_empty() { "root" } else { &value }).with_context(|| {
                format!(
                    "unknown service account in {}: User={value:?}",
                    unit_path.display()
                )
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(PreviousAccount::NotInstalled)
        }
        Err(error) => {
            Err(error).with_context(|| format!("reading installed unit {}", unit_path.display()))
        }
    }
}

pub(super) struct AccountChange {
    pub(super) previous: UserName,
    pub(super) new: UserName,
}

pub(super) fn account_change(
    previous: PreviousAccount,
    new_user: Option<String>,
) -> Result<Option<AccountChange>> {
    let explicit_root = new_user.as_deref() == Some("root");
    let new = UserName::parse(new_user.as_deref().unwrap_or("root"))?;
    let previous = match previous {
        PreviousAccount::NotInstalled => return Ok(None),
        PreviousAccount::Root => UserName::parse("root")?,
        PreviousAccount::User(user) => user,
    };
    if (previous.as_str() == "root" || new.as_str() == "root") && !explicit_root {
        bail!("refusing ownership repair from User={previous} to rendered User={new}: a root account requires explicit User=root in the rendered unit; repair ownership manually before changing the service account");
    }
    Ok((previous != new).then_some(AccountChange { previous, new }))
}

/// Ask systemd for the effective account and its drop-ins. Refuse conflicting
/// User overrides before planning any repair, naming the file and both users.
fn effective_previous_account(
    target: &Path,
    service: &str,
    rendered_user: &UserName,
) -> Result<PreviousAccount> {
    let on_disk = match std::fs::read_to_string(target) {
        Ok(unit) => Some(unit),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading installed unit {}", target.display()))
        }
    };
    let show = |property: &str| -> Result<String> {
        let output = std::process::Command::new("systemctl")
            .args(["show", "-p", property, "--value", service])
            .output()
            .with_context(|| format!("reading systemd {property}"))?;
        if !output.status.success() {
            bail!("cannot determine effective systemd {property} for {service}");
        }
        Ok(String::from_utf8(output.stdout)
            .context("systemd property is not UTF-8")?
            .trim_end_matches('\n')
            .to_string())
    };
    let load_state = match show("LoadState") {
        Ok(state) => Some(state),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            None
        }
        Err(error) => return Err(error),
    };
    let effective = if load_state.as_deref() == Some("loaded") {
        // Unlike a missing User= directive, empty command output is unknown.
        let value = show("User")?;
        Some(account(&value).with_context(|| {
            format!("cannot determine effective account for {service} at {}: systemctl show -p User --value {service} returned unsupported User={value:?}", target.display())
        })?)
    } else {
        None
    };
    let reported = if load_state.is_some() {
        show("DropInPaths")?
    } else {
        String::new()
    };
    let reported: Vec<PathBuf> = reported.split_whitespace().map(PathBuf::from).collect();
    // A first install can still have persistent drop-ins. Also inspect the
    // standard search directories so newly added overrides cannot hide in
    // systemd's cached property values until daemon-reload.
    let mut drop_in_dirs = vec![format!("{service}.d")];
    if let Some(stem) = service.strip_suffix(".service") {
        drop_in_dirs.extend(
            stem.match_indices('-')
                .map(|(index, _)| format!("{}.service.d", &stem[..=index]))
                .rev(),
        );
    }
    drop_in_dirs.push("service.d".into());
    let bases: Vec<_> = target
        .parent()
        .into_iter()
        .chain([
            Path::new("/etc/systemd/system"),
            Path::new("/run/systemd/system"),
            Path::new("/usr/local/lib/systemd/system"),
            Path::new("/usr/lib/systemd/system"),
            Path::new("/lib/systemd/system"),
        ])
        .collect();
    // Select the highest-priority file for each name, then apply names in
    // lexical order, just as systemd does across its drop-in directories.
    let mut drop_ins = BTreeMap::new();
    for directory in &drop_in_dirs {
        for base in &bases {
            match std::fs::read_dir(base.join(directory)) {
                Ok(entries) => {
                    for entry in entries {
                        let entry = entry?;
                        let path = entry.path();
                        if path
                            .extension()
                            .is_some_and(|extension| extension == "conf")
                        {
                            drop_ins.entry(entry.file_name()).or_insert(path);
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "reading systemd drop-ins in {}",
                            base.join(directory).display()
                        )
                    })
                }
            }
        }
    }
    for path in reported {
        if let Some(name) = path.file_name() {
            drop_ins.entry(name.to_owned()).or_insert(path);
        }
    }
    let mut override_user = None;
    for path in drop_ins.into_values() {
        let body = std::fs::read_to_string(&path)
            .with_context(|| format!("reading systemd drop-in {}", path.display()))?;
        if let Some(value) = unit_user(&body) {
            override_user = Some((path, value));
        }
    }
    if let Some((path, value)) = &override_user {
        let user =
            UserName::parse(if value.is_empty() { "root" } else { value }).with_context(|| {
                format!(
                    "unknown service account in systemd drop-in {}: User={value:?}",
                    path.display()
                )
            })?;
        if user != *rendered_user {
            bail!("refusing to change the service account: systemd drop-in {} sets User={user} but the rendered unit sets User={rendered_user}; the drop-in survives the base unit update; adjust that drop-in first", path.display());
        }
    }
    if load_state.is_none() {
        if on_disk.is_none() {
            return Ok(PreviousAccount::NotInstalled);
        }
        return match override_user {
            Some((_, value)) => account(if value.is_empty() { "root" } else { &value }),
            None => previous_account(target),
        };
    }
    let load_state = load_state.unwrap();
    if load_state == "not-found" && on_disk.is_none() {
        return Ok(PreviousAccount::NotInstalled);
    }
    if load_state != "loaded" {
        bail!("cannot determine effective account for {service}: systemd LoadState={load_state}");
    }
    // Read the fragment as well, including vendor units outside /etc. Never
    // let a successful show hide an unreadable or vanished base unit.
    let fragment = show("FragmentPath")?;
    if fragment.is_empty() {
        bail!("systemd did not report a readable installed fragment for {service}");
    }
    std::fs::read_to_string(&fragment)
        .with_context(|| format!("reading installed systemd fragment {fragment}"))?;
    effective.context("systemd did not report the effective account")
}

/// A read-only plan shared by update preflight and the eventual unit swap.
struct ServerUnit {
    target: PathBuf,
    rendered: String,
    change: Option<AccountChange>,
    managed: ServerManagedPaths,
}

impl ServerUnit {
    /// Validate the complete account transition without changing any artifacts.
    fn preflight(
        repo: &Path,
        service: &str,
        units_dir: &Path,
        repair: bool,
    ) -> Result<Option<ServerUnit>> {
        let mut components = Path::new(service).components();
        if !service.ends_with(".service")
            || !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
        {
            bail!("invalid systemd service name '{service}'");
        }
        if !super::systemd_available() {
            return Ok(None);
        }
        let source = repo.join("packaging/systemd/gah-server.service");
        let template = std::fs::read_to_string(&source)
            .with_context(|| format!("reading systemd unit template {}", source.display()))?;
        let values = super::units::UnitValues::resolve(repo)?;
        let rendered =
            super::units::render(&template, &values).context("rendering gah-server.service")?;
        let target = units_dir.join(service);
        let new_user = unit_user(&rendered);
        let rendered_user = UserName::parse(
            new_user
                .as_deref()
                .filter(|user| !user.is_empty())
                .unwrap_or("root"),
        )?;
        let previous = effective_previous_account(&target, service, &rendered_user)?;
        // The effective drop-in User= agrees with the rendered account after reload.
        let change = account_change(previous, new_user)
            .with_context(|| format!("planning ownership repair for {service}"))?;
        let managed = if change.is_some() {
            server_managed_paths(repo, values.config_path())?
        } else {
            ServerManagedPaths {
                paths: vec![],
                anchors: vec![],
            }
        };
        if let Some(change) = &change {
            if !repair {
                repair_or_refuse(repo, change, &managed, false)?;
            }
        }
        Ok(Some(ServerUnit {
            target,
            rendered,
            change,
            managed,
        }))
    }

    fn repair(&self, repo: &Path, repair: bool) -> Result<()> {
        if let Some(change) = &self.change {
            repair_or_refuse(repo, change, &self.managed, repair)?;
        }
        Ok(())
    }

    fn install(self, repo: &Path) -> Result<PathBuf> {
        let staged = tempfile::NamedTempFile::new().context("staging gah-server.service")?;
        std::fs::write(staged.path(), self.rendered).context("staging gah-server.service")?;
        super::run_command(
            repo,
            "sudo",
            &[
                "install",
                "-o",
                "root",
                "-g",
                "root",
                "-m",
                "0644",
                staged
                    .path()
                    .to_str()
                    .context("staged systemd unit path is not UTF-8")?,
                self.target
                    .to_str()
                    .context("systemd unit target path is not UTF-8")?,
            ],
        )?;
        super::run_command(repo, "sudo", &["systemctl", "daemon-reload"])?;
        Ok(self.target)
    }
}

/// Decide whether this update may proceed before fetching, installing assets,
/// replacing binaries, building dist, or changing the unit.
pub(super) fn preflight(
    repo: &Path,
    role: super::HostRole,
    service: &str,
    repair: bool,
) -> Result<()> {
    if matches!(role, super::HostRole::Central | super::HostRole::Standalone) {
        ServerUnit::preflight(repo, service, Path::new("/etc/systemd/system"), repair)?;
    }
    Ok(())
}

pub(super) fn install_server_unit_into(
    repo: &Path,
    service: &str,
    units_dir: &Path,
    repair: bool,
) -> Result<Option<PathBuf>> {
    let Some(unit) = ServerUnit::preflight(repo, service, units_dir, repair)? else {
        return Ok(None);
    };
    unit.repair(repo, repair)?;
    unit.install(repo).map(Some)
}

/// Candidate paths and a closed set of trusted checkout/config boundaries.
#[derive(Debug)]
pub(super) struct ServerManagedPaths {
    pub(super) paths: Vec<PathBuf>,
    pub(super) anchors: Vec<PathBuf>,
}

/// Reject system trees and broad roots even when config explicitly names
/// them. Callers must pass canonical paths so aliases cannot evade this rule.
fn protected_root(path: &Path) -> bool {
    if [
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
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
    {
        return true;
    }
    if path
        .components()
        .filter(|part| matches!(part, std::path::Component::Normal(_)))
        .count()
        < 2
    {
        return true;
    }
    if ["/home", "/Users", "/root"]
        .iter()
        .any(|root| std::fs::canonicalize(root).is_ok_and(|root| path == root))
    {
        return true;
    }
    if path
        .parent()
        .is_some_and(|parent| parent == Path::new("/home") || parent == Path::new("/Users"))
    {
        return true;
    }
    if std::env::var_os("HOME")
        .and_then(|home| std::fs::canonicalize(home).ok())
        .is_some_and(|home| home == path)
    {
        return true;
    }
    let Ok(passwd) = std::fs::read_to_string("/etc/passwd") else {
        return true;
    };
    if passwd
        .lines()
        .filter_map(|line| line.split(':').nth(5))
        .filter(|home| !home.is_empty())
        .any(|home| std::fs::canonicalize(home).is_ok_and(|home| home == path))
    {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(target), Some(parent)) = (std::fs::metadata(path), path.parent()) {
            if std::fs::metadata(parent).map_or(true, |parent| target.dev() != parent.dev()) {
                return true;
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        // Device comparisons miss bind mounts. Also refuse a recursive target
        // containing any nested mount so chown cannot cross filesystems.
        let Ok(mounts) = std::fs::read_to_string("/proc/self/mountinfo") else {
            return true;
        };
        for mount in mounts
            .lines()
            .filter_map(|line| line.split_whitespace().nth(4))
        {
            let mount = mount
                .replace("\\040", " ")
                .replace("\\011", "\t")
                .replace("\\012", "\n")
                .replace("\\134", "\\");
            if Path::new(&mount).starts_with(path) {
                return true;
            }
        }
    }
    false
}

/// Prove this is a checkout of the updater's repository before trusting any
/// descendant. A .git entry prevents Git from discovering a parent checkout.
fn checkout_anchor(repo: &Path) -> Result<PathBuf> {
    let checkout = std::fs::canonicalize(repo)
        .with_context(|| format!("resolving ownership checkout boundary {}", repo.display()))?;
    if protected_root(&checkout) {
        bail!("refusing ownership checkout {} (checkout {}): protected system, home, or mount boundary", checkout.display(), repo.display());
    }
    let git = checkout.join(".git");
    if !git.is_dir() && !git.is_file() {
        bail!(
            "refusing ownership checkout {}: missing .git directory or gitfile",
            checkout.display()
        );
    }
    let mut command = std::process::Command::new("git");
    command
        .arg("-C")
        .arg(&checkout)
        .args(["config", "--get", "remote.origin.url"]);
    let output = crate::runner::process::run_bounded(
        command, std::time::Duration::from_millis(250),
    ).with_context(|| format!("refusing ownership checkout {}: cannot prove origin repository (git probe failed or timed out)", checkout.display()))?;
    if !output.status.success() {
        bail!(
            "refusing ownership checkout {}: cannot prove origin repository",
            checkout.display()
        );
    }
    let origin = String::from_utf8(output.stdout).context("origin URL is not UTF-8")?;
    let origin = origin.trim();
    // Accept URL forms only: a similarly named local remote is not upstream.
    let origin = origin.to_ascii_lowercase();
    let repository = origin
        .strip_prefix("https://github.com/")
        .or_else(|| origin.strip_prefix("git@github.com:"))
        .or_else(|| origin.strip_prefix("ssh://git@github.com/"))
        .map(|path| path.strip_suffix(".git").unwrap_or(path));
    const EXPECTED_REPOSITORY: &str = "github.com/kh1ng/git-agent-harness";
    if repository != Some("kh1ng/git-agent-harness") {
        bail!("refusing ownership checkout {}: origin does not match expected repository {EXPECTED_REPOSITORY}", checkout.display());
    }
    Ok(checkout)
}

/// A configured path alone is not proof of state. Require the unit's exact
/// config beneath it, or an existing GAH state marker that is not a symlink.
fn prove_state_anchor(root: &Path, config_path: &Path) -> Result<()> {
    if std::fs::canonicalize(config_path)
        .is_ok_and(|config| config.is_file() && config.starts_with(root))
        || ["ledger.jsonl", "events.jsonl", "reconciliation.jsonl"]
            .iter()
            .any(|marker| {
                root.join(marker)
                    .symlink_metadata()
                    .is_ok_and(|meta| meta.is_file())
            })
        || root
            .join("sessions")
            .symlink_metadata()
            .is_ok_and(|meta| meta.is_dir())
    {
        return Ok(());
    }
    bail!("refusing ownership state boundary {}: contains neither unit config {} nor known GAH state markers", root.display(), config_path.display());
}

/// State boundaries are never chowned. Refuse before deployment if the
/// installing account cannot create state beneath the unchanged boundary.
fn ensure_state_boundary_access(root: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(root.as_os_str().as_bytes())?;
        // SAFETY: path is a valid NUL-terminated string; access only checks
        // the invoking account's permissions and does not change the path.
        if unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::W_OK | libc::X_OK) } != 0 {
            bail!("state boundary {} is not accessible to the installing account; ownership repair only covers strict descendants, so repair this boundary's permissions manually before updating", root.display());
        }
    }
    Ok(())
}

/// Bound read-only filesystem validation, including metadata on automounts or
/// TCC-protected paths. A stalled OS call cannot be cancelled; its worker may
/// finish later, so probes must never mutate files or launch unbounded commands.
fn read_only_probe<T: Send + 'static>(
    path: &Path,
    probe: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("ownership-probe".into())
        .spawn(move || {
            let _ = sender.send(probe());
        })
        .context("starting read-only ownership probe")?;
    receiver
        .recv_timeout(std::time::Duration::from_millis(750))
        .with_context(|| {
            format!(
                "ownership boundary probe failed or timed out for {}",
                path.display()
            )
        })?
}

/// Collect state boundaries from the exact file named by the rendered unit.
/// Environment variables can nominate candidates, never authorize targets.
pub(super) fn server_managed_paths(repo: &Path, config_path: &Path) -> Result<ServerManagedPaths> {
    let repo_owned = repo.to_path_buf();
    let config_owned = config_path.to_path_buf();
    read_only_probe(repo, move || {
        collect_managed_paths(&repo_owned, &config_owned)
    })
}

fn collect_managed_paths(repo: &Path, config_path: &Path) -> Result<ServerManagedPaths> {
    let checkout = checkout_anchor(repo)?;
    let mut state_roots = BTreeSet::new();
    match std::fs::read_to_string(config_path) {
        Ok(text) => {
            // Read this file directly: config loading can merge environment or
            // canonical routing, neither of which authorizes ownership roots.
            let config: crate::config::GahConfig = toml::from_str(&text).with_context(|| {
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
                if !root.trim().is_empty() {
                    let root = PathBuf::from(root.trim());
                    if !root.is_absolute()
                        || root
                            .components()
                            .any(|part| part == std::path::Component::ParentDir)
                    {
                        bail!(
                            "state root must be an absolute path without '..': {}",
                            root.display()
                        );
                    }
                    state_roots.insert(root);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading state roots from {}", config_path.display()))
        }
    }
    let mut paths = Vec::new();
    let mut anchors = vec![checkout];
    for root in state_roots.iter().filter(|root| {
        !state_roots
            .iter()
            .any(|other| other != *root && root.starts_with(other))
    }) {
        paths.push(root.clone());
        let metadata = match root.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("proving state boundary {}", root.display()))
            }
            Ok(meta) => meta,
        };
        let anchor = std::fs::canonicalize(root)
            .with_context(|| format!("resolving ownership state boundary {}", root.display()))?;
        if protected_root(&anchor) {
            bail!(
                "refusing ownership state boundary {}: protected system, home, or mount boundary",
                anchor.display()
            );
        }
        if metadata.file_type().is_symlink() {
            bail!("refusing ownership state boundary {} (configured {}): symlink cannot prove a state root", anchor.display(), root.display());
        }
        prove_state_anchor(&anchor, config_path)?;
        ensure_state_boundary_access(root)?;
        anchors.push(anchor);
        // A boundary itself is never a recursive target. Repair each child,
        // including files directly under the state root, instead.
        for entry in std::fs::read_dir(root).with_context(|| {
            format!("listing state root {} for ownership repair", root.display())
        })? {
            paths.push(entry?.path());
        }
    }
    paths.extend([repo.join("config"), repo.join("apps/server/dist")]);
    if let Some(web) = super::resolve_web_deploy_root(std::env::var_os("GAH_WEB_DEPLOY_ROOT"))? {
        paths.push(web);
    }
    Ok(ServerManagedPaths { paths, anchors })
}

/// A candidate must resolve strictly inside a trusted boundary. Broad roots,
/// mount roots and home roots remain denied even if inside that boundary.
fn confirmed_chown_target(path: &Path, anchors: &[PathBuf]) -> Result<Option<PathBuf>> {
    let candidate = path.to_path_buf();
    let anchors = anchors.to_vec();
    read_only_probe(path, move || {
        let Ok(resolved) = std::fs::canonicalize(candidate) else {
            return Ok(None);
        };
        if protected_root(&resolved)
            || !anchors
                .iter()
                .any(|anchor| resolved != *anchor && resolved.starts_with(anchor))
        {
            return Ok(None);
        }
        Ok(Some(resolved))
    })
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
        let Some(target) = confirmed_chown_target(path, &managed.anchors)? else {
            let candidate = path.to_path_buf();
            if read_only_probe(path, move || Ok(candidate.try_exists()?)).unwrap_or(false) {
                eprintln!(
                    "[gah update] skipped ownership repair for {}: it is a protected root, or it \
                     resolves outside the trusted checkout and state boundaries. Inspect it, then run \
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
        super::run_command(
            repo,
            "sudo",
            &["chown", "-R", "-h", "-P", "--", change.new.as_str(), target],
        )?;
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
mod tests;
