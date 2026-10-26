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
/// resets systemd to root; directives in other sections have no effect.
pub(super) fn unit_user(unit: &str) -> Option<String> {
    let mut service = false;
    let mut user = None;
    for line in unit.lines().map(str::trim) {
        if line.starts_with('[') {
            service = line == "[Service]";
        } else if service {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "User" {
                    user = Some(value.trim().to_string());
                }
            }
        }
    }
    user
}

fn account(value: &str) -> Result<PreviousAccount> {
    let user = UserName::parse(if value.is_empty() { "root" } else { value })?;
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
        Ok(unit) => account(unit_user(&unit).as_deref().unwrap_or("root")),
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
    let new = UserName::parse(new_user.as_deref().unwrap_or("root"))?;
    let previous = match previous {
        PreviousAccount::NotInstalled => return Ok(None),
        PreviousAccount::Root => UserName::parse("root")?,
        PreviousAccount::User(user) => user,
    };
    Ok((previous != new).then_some(AccountChange { previous, new }))
}

/// Ask systemd for the effective account and its drop-ins. Refuse any User
/// override that will survive the base-unit swap, naming the responsible file.
fn effective_previous_account(target: &Path, service: &str) -> Result<PreviousAccount> {
    let on_disk = previous_account(target)?;
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
            .trim()
            .to_string())
    };
    let load_state = show("LoadState")?;
    let effective = if load_state == "loaded" {
        Some(account(&show("User")?)?)
    } else {
        None
    };
    let mut drop_ins: BTreeSet<PathBuf> = show("DropInPaths")?
        .split_whitespace()
        .map(PathBuf::from)
        .collect();
    // A first install can still have persistent drop-ins. Also inspect the
    // standard search directories so newly added overrides cannot hide in
    // systemd's cached property values until daemon-reload.
    let mut drop_in_dirs = vec![format!("{service}.d"), "service.d".into()];
    if let Some(stem) = service.strip_suffix(".service") {
        drop_in_dirs.extend(
            stem.match_indices('-')
                .map(|(index, _)| format!("{}.service.d", &stem[..=index])),
        );
    }
    for base in target.parent().into_iter().chain([
        Path::new("/etc/systemd/system"),
        Path::new("/run/systemd/system"),
        Path::new("/usr/local/lib/systemd/system"),
        Path::new("/usr/lib/systemd/system"),
        Path::new("/lib/systemd/system"),
    ]) {
        for directory in &drop_in_dirs {
            match std::fs::read_dir(base.join(directory)) {
                Ok(entries) => {
                    for entry in entries {
                        let path = entry?.path();
                        if path
                            .extension()
                            .is_some_and(|extension| extension == "conf")
                        {
                            drop_ins.insert(path);
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
    for path in drop_ins {
        let body = std::fs::read_to_string(&path)
            .with_context(|| format!("reading systemd drop-in {}", path.display()))?;
        if unit_user(&body).is_some() {
            bail!("refusing to change the service account: systemd drop-in {} overrides User= and survives the base unit update; adjust that drop-in first", path.display());
        }
    }
    if load_state == "not-found" && matches!(on_disk, PreviousAccount::NotInstalled) {
        return Ok(on_disk);
    }
    if load_state != "loaded" {
        bail!("cannot determine effective account for {service}: systemd LoadState={load_state}");
    }
    // Read the fragment as well, including vendor units outside /etc. Never
    // let a successful show hide an unreadable or vanished base unit.
    let fragment = show("FragmentPath")?;
    if fragment.is_empty()
        || matches!(
            previous_account(Path::new(&fragment))?,
            PreviousAccount::NotInstalled
        )
    {
        bail!("systemd did not report a readable installed fragment for {service}");
    }
    effective.context("systemd did not report the effective account")
}

/// A read-only plan shared by update preflight and the eventual unit swap.
struct ServerUnit {
    target: PathBuf,
    rendered: String,
    change: Option<AccountChange>,
    managed: ServerManagedPaths,
}

fn prepare_server_unit(
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
    let previous = effective_previous_account(&target, service)?;
    // No User= drop-in is allowed above, so the rendered base account is also
    // the effective new account after daemon-reload.
    let change = account_change(previous, unit_user(&rendered))?;
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

/// Decide whether this update may proceed before fetching, installing assets,
/// replacing binaries, building dist, or changing the unit.
pub(super) fn preflight(
    repo: &Path,
    role: super::HostRole,
    service: &str,
    repair: bool,
) -> Result<()> {
    if matches!(role, super::HostRole::Central | super::HostRole::Standalone) {
        prepare_server_unit(repo, service, Path::new("/etc/systemd/system"), repair)?;
    }
    Ok(())
}

pub(super) fn install_server_unit_into(
    repo: &Path,
    service: &str,
    units_dir: &Path,
    repair: bool,
) -> Result<Option<PathBuf>> {
    let Some(unit) = prepare_server_unit(repo, service, units_dir, repair)? else {
        return Ok(None);
    };
    if let Some(change) = &unit.change {
        repair_or_refuse(repo, change, &unit.managed, repair)?;
    }
    let staged = tempfile::NamedTempFile::new().context("staging gah-server.service")?;
    std::fs::write(staged.path(), unit.rendered).context("staging gah-server.service")?;
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
            unit.target
                .to_str()
                .context("systemd unit target path is not UTF-8")?,
        ],
    )?;
    super::run_command(repo, "sudo", &["systemctl", "daemon-reload"])?;
    Ok(Some(unit.target))
}

/// Candidate paths and a closed set of trusted checkout/config boundaries.
#[derive(Debug)]
pub(super) struct ServerManagedPaths {
    pub(super) paths: Vec<PathBuf>,
    pub(super) anchors: Vec<PathBuf>,
}

/// Reject broad roots even when config explicitly names them. Mount roots
/// and account home roots must never be recursive ownership targets.
fn protected_root(path: &Path) -> bool {
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

/// Collect state boundaries from the exact file named by the rendered unit.
/// Environment variables can nominate candidates, never authorize targets.
pub(super) fn server_managed_paths(repo: &Path, config_path: &Path) -> Result<ServerManagedPaths> {
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
    let mut anchors =
        vec![std::fs::canonicalize(repo).context("resolving ownership checkout boundary")?];
    for root in state_roots.iter().filter(|root| {
        !state_roots
            .iter()
            .any(|other| other != *root && root.starts_with(other))
    }) {
        paths.push(root.clone());
        // Keep the configured basename: canonicalizing this candidate itself
        // would let a state-root symlink authorize its own escape.
        let Some(anchor) = root
            .parent()
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .zip(root.file_name())
            .map(|(parent, name)| parent.join(name))
        else {
            continue;
        };
        if protected_root(&anchor)
            || root
                .symlink_metadata()
                .map_or(true, |meta| meta.file_type().is_symlink())
        {
            continue;
        }
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
fn confirmed_chown_target(path: &Path, anchors: &[PathBuf]) -> Option<PathBuf> {
    let resolved = std::fs::canonicalize(path).ok()?;
    if protected_root(&resolved)
        || !anchors
            .iter()
            .any(|anchor| resolved != *anchor && resolved.starts_with(anchor))
    {
        return None;
    }
    Some(resolved)
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
