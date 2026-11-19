//! Update a Linux control plane deployed from an archive. The service manager
//! owns its unit files and paths. Only the running application's files change.
use super::{checked_manifest, install_release_artifacts, AssetLocator};
use crate::update::{self, UpdateArgs};
use anyhow::{bail, ensure, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const SERVER: &str = "apps/server/dist";
const WEB: &str = "apps/web/dist";
const EDGE: &str = "https://github.com/Kh1ng/Parasaur/releases/download/edge/edge-manifest.json";

fn installed(root: &Path) -> bool {
    !root.join(".git").exists()
        && root.join(SERVER).join("bin.js").is_file()
        && root.join(WEB).join("index.html").is_file()
        && fs::read(root.join("package.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .is_some_and(|p| p["name"] == "git-agent-harness")
}

pub(in crate::update) fn update_if_installed(args: &UpdateArgs) -> Result<bool> {
    let cwd = std::env::current_dir()?;
    let root = args.repo.as_deref().unwrap_or(&cwd);
    if !installed(root) {
        return Ok(false);
    }
    let root = root.canonicalize()?;
    let _lock = update::lock_update_path(&root, &root.join(".gah-update.lock"))?;
    let _maintenance = crate::agents::maintenance_lock()?;
    if args.restart_server {
        ensure!(
            args.server_service.ends_with(".service")
                && !args.server_service.starts_with('-')
                && Path::new(&args.server_service).components().count() == 1,
            "invalid control-plane service name"
        );
        update::ensure_no_running_loop_before_server_restart()?;
    }
    let cli = update::installed_binary_path()?;
    let deployed = root.join("bin/gah");
    if let Some(pinned) = std::env::var_os("PARASAUR_BINARY") {
        let pinned =
            fs::canonicalize(&pinned).context("resolving the configured PARASAUR_BINARY")?;
        ensure!(
            [&cli, &deployed]
                .iter()
                .any(|path| fs::canonicalize(path).is_ok_and(|path| path == pinned)),
            "the configured PARASAUR_BINARY is outside the release CLI install paths"
        );
    }
    let configured_manifest = std::env::var("PARASAUR_RELEASE_MANIFEST").ok();
    let location = super::resolve_manifest_location(
        &root,
        Some(
            args.release_manifest
                .as_deref()
                .or(configured_manifest.as_deref())
                .unwrap_or(EDGE),
        ),
    )?;
    let web_root = update::resolve_web_deploy_root(std::env::var_os("PARASAUR_WEB_DEPLOY_ROOT"))?;
    let manifest = checked_manifest(&location)?;
    for asset in &manifest.assets {
        ensure!(
            Path::new(&asset.name).file_name().and_then(|s| s.to_str())
                == Some(asset.name.as_str())
                && asset
                    .sha256
                    .as_ref()
                    .is_some_and(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit())),
            "release asset requires a filename and SHA-256: {}",
            asset.name
        );
    }
    let stage = tempfile::Builder::new()
        .prefix(".gah-release-")
        .tempdir_in(&root)?;
    if root.join("package-lock.json").is_file() {
        fs::copy(
            root.join("package-lock.json"),
            stage.path().join("package-lock.json"),
        )?;
    }
    let staged_cli = stage.path().join("bin/gah");
    let bundle = install_release_artifacts(
        &manifest,
        &AssetLocator::new(location),
        stage.path(),
        args.role,
        staged_cli.clone(),
    )?
    .context("control-plane release is missing its server bundle")?;
    // Prepare all destinations on their own filesystems, including installations
    // where the loop and the server invoke two separate copies of the CLI.
    let bin_dir = cli.parent().context("CLI has no install directory")?;
    fs::create_dir_all(bin_dir)?;
    let mut replacements = Vec::new();
    let primary = if cli.is_symlink() {
        let resolved = cli
            .canonicalize()
            .context("resolving the installed loop CLI")?;
        ensure!(
            resolved.starts_with(bin_dir.canonicalize()?) || resolved.starts_with(&root),
            "the installed loop CLI points outside its release and binary directories"
        );
        resolved
    } else {
        bin_dir.canonicalize()?.join("gah")
    };
    replacements.push(Replacement::file(&staged_cli, &primary)?);
    for name in ["gah-mcp-server", "parasaur"] {
        replacements.push(Replacement::file(
            &staged_cli.with_file_name(name),
            &bin_dir.join(name),
        )?);
    }
    if deployed.is_file() && deployed.canonicalize()? != primary {
        let path = deployed.canonicalize()?;
        ensure!(
            path.starts_with(&root),
            "the deployed CLI points outside its release directory"
        );
        replacements.push(Replacement::file(&staged_cli, &path)?);
    }
    for relative in [SERVER, WEB] {
        let destination = root.join(relative).canonicalize()?;
        ensure!(
            destination.starts_with(&root),
            "deployed application output escapes the release directory"
        );
        replacements.push(Replacement::directory(
            &stage.path().join(relative),
            &destination,
        )?);
    }
    let config = update::user_config_home()?.join("opencode/agents");
    let install_opencode = args.agents.iter().any(|name| name == "opencode");
    if install_opencode {
        fs::create_dir_all(&config)?;
    }
    let agents = bundle.path().join("packaging/opencode/agents");
    for agent in ["gah-implementer.md", "gah-reviewer.md"] {
        let destination = config.join(agent);
        if install_opencode || destination.is_file() {
            replacements.push(Replacement::file(&agents.join(agent), &destination)?);
        }
    }
    if args.restart_server {
        update::ensure_no_running_loop_before_server_restart()?;
    }
    commit(&mut replacements, || {
        if update::installation::quota_refresh_selected(&args.agents) {
            update::install_quota_refresh_unit_template(&root)?;
        }
        if let Some(destination) = web_root {
            update::deploy_web_dist(&root, &root.join(WEB), destination)?;
        }
        Ok(())
    })?;
    // A restart requested by the dashboard may terminate the updater's cgroup.
    // Finish cleaning its staged files while the old process is still running.
    drop(replacements);
    drop(bundle);
    drop(stage);
    println!(
        "Installed control plane {} ({}) in {}",
        manifest.version,
        manifest.commit.as_deref().unwrap_or(&manifest.channel),
        root.display()
    );
    if args.restart_server {
        update::run_command(
            &root,
            "sudo",
            &["systemctl", "restart", "--", &args.server_service],
        )?;
        update::run_command(
            &root,
            "systemctl",
            &["is-active", "--quiet", "--", &args.server_service],
        )?;
    }
    Ok(true)
}

/// Keep the previous contents next to their destination until every replacement
/// succeeds. A failure on a later filesystem can then restore the earlier ones.
struct Replacement {
    temp: tempfile::TempDir,
    target: PathBuf,
    had_previous: bool,
}

impl Replacement {
    fn new(target: &Path, directory: bool) -> Result<Self> {
        let parent = target.parent().context("missing destination parent")?;
        let had_previous = match fs::symlink_metadata(target) {
            Ok(meta) => {
                ensure!(
                    if directory {
                        meta.is_dir()
                    } else {
                        meta.is_file() || meta.is_symlink()
                    },
                    "unexpected release destination type: {}",
                    target.display()
                );
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            target: target.to_owned(),
            had_previous,
            temp: tempfile::Builder::new()
                .prefix(".gah-swap-")
                .tempdir_in(parent)?,
        })
    }

    fn file(source: &Path, target: &Path) -> Result<Self> {
        let replacement = Self::new(target, false)?;
        let next = replacement.temp.path().join("next");
        if source.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(source)?, next)?;
        } else {
            fs::copy(source, next)?;
        }
        Ok(replacement)
    }

    fn directory(source: &Path, target: &Path) -> Result<Self> {
        let replacement = Self::new(target, true)?;
        fs::rename(source, replacement.temp.path().join("next"))?;
        Ok(replacement)
    }

    fn swap(&self) -> Result<()> {
        let next = self.temp.path().join("next");
        if !self.had_previous {
            fs::rename(next, &self.target)?;
            return Ok(());
        }
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let left = CString::new(next.as_os_str().as_bytes())?;
        let right = CString::new(self.target.as_os_str().as_bytes())?;
        // SAFETY: both paths are valid, NUL-terminated and live across the syscall.
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                left.as_ptr(),
                libc::AT_FDCWD,
                right.as_ptr(),
                libc::RENAME_EXCHANGE,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("exchanging {}", self.target.display()));
        }
        Ok(())
    }
}

fn commit(replacements: &mut [Replacement], finish: impl FnOnce() -> Result<()>) -> Result<()> {
    let mut installed = 0;
    let result = replacements
        .iter()
        .try_for_each(|item| {
            item.swap()?;
            installed += 1;
            Ok(())
        })
        .and_then(|()| finish());
    if let Err(error) = result {
        for previous in replacements[..installed].iter_mut().rev() {
            let rollback = if previous.had_previous {
                previous.swap()
            } else {
                fs::rename(&previous.target, previous.temp.path().join("next"))
                    .map_err(anyhow::Error::from)
            };
            if let Err(rollback) = rollback {
                // Never discard the backup when restoration failed.
                previous.temp.disable_cleanup(true);
                eprintln!(
                    "Restore {} from {}: {rollback}",
                    previous.target.display(),
                    previous.temp.path().display()
                );
            }
        }
        bail!("applying control-plane release: {error:#}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_activation_restores_existing_outputs_and_removes_new_alias() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("next-cli");
        let cli = root.path().join("gah");
        let alias = root.path().join("parasaur");
        let web = root.path().join("web");
        let new_web = root.path().join("new-web");
        fs::create_dir(&web).unwrap();
        fs::create_dir(&new_web).unwrap();
        fs::write(web.join("index.html"), "before").unwrap();
        fs::write(new_web.join("index.html"), "after").unwrap();
        fs::write(&source, "new CLI").unwrap();
        fs::write(&cli, "old CLI").unwrap();
        let mut changes = [
            Replacement::file(&source, &cli).unwrap(),
            Replacement::file(&source, &alias).unwrap(),
            Replacement::directory(&new_web, &web).unwrap(),
        ];
        let error = commit(&mut changes, || {
            assert_eq!(fs::read_to_string(&cli)?, "new CLI");
            assert_eq!(fs::read_to_string(web.join("index.html"))?, "after");
            bail!("external dashboard deployment failed")
        })
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("external dashboard deployment failed"));
        assert_eq!(fs::read_to_string(cli).unwrap(), "old CLI");
        assert_eq!(
            fs::read_to_string(web.join("index.html")).unwrap(),
            "before"
        );
        assert!(!alias.exists());
    }
}
