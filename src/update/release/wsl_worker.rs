//! Windows enrollment installs a versioned WSL runtime outside Git. Stage the
//! complete matching kit next to it, then switch what the launchers source.
use super::*;
use crate::installer::files::{replace, shell_quote};
use anyhow::ensure;
use std::fs;

const EDGE: &str = "https://github.com/Kh1ng/Parasaur/releases/download/edge/edge-manifest.json";
const KIT: &str = "apps/server/dist/worker-artifacts";

pub(in crate::update) fn update_if_installed(args: &crate::update::UpdateArgs) -> Result<bool> {
    let home = PathBuf::from(env::var_os("HOME").context("HOME is required")?);
    let root = home.join(".local/share/gah/worker");
    if !root.join("worker.env").is_file() || !root.join("identity.json").is_file() {
        return Ok(false);
    }
    let root = root.canonicalize()?;
    let installed = |dir: &Path| {
        dir.parent() == Some(root.as_path())
            && dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("release."))
    };
    let cwd = match &args.repo {
        Some(dir) => dir.canonicalize()?,
        None => env::current_dir()?,
    };
    if !installed(&cwd)
        && (args.repo.is_some()
            || !env::current_exe()?
                .ancestors()
                .nth(2)
                .is_some_and(installed))
    {
        return Ok(false);
    }
    let _lock = crate::update::worker_maintenance_lock()?;
    let config = crate::config::load(None)?;
    let role = crate::node_role::NodeRoleStatus::resolve(&config.defaults)?;
    ensure!(
        role.role == HostRole::Worker,
        "This installation is not configured as a worker"
    );
    let central = role.central_url.context("The worker has no central URL")?;
    let location =
        resolve_manifest_location(&cwd, Some(args.release_manifest.as_deref().unwrap_or(EDGE)))?;
    let manifest: ReleaseManifest = serde_json::from_slice(&read_manifest(&location)?)?;
    ensure!(
        manifest.schema == 1,
        "Unsupported release manifest schema {}",
        manifest.schema
    );
    ensure!(
        compare_release_versions(&manifest.version, env!("CARGO_PKG_VERSION")) != Ordering::Less,
        "Refusing to downgrade the installed worker to {}",
        manifest.version
    );
    install(
        &root,
        &central,
        &manifest,
        &AssetLocator::new(location),
        &args.agents,
    )?;
    Ok(true)
}

fn install(
    root: &Path,
    central: &str,
    manifest: &ReleaseManifest,
    locator: &AssetLocator,
    agents: &[String],
) -> Result<()> {
    let revision = manifest
        .commit
        .as_deref()
        .context("Worker update requires a release commit")?;
    ensure!(
        revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid worker release commit"
    );
    let bundle = manifest
        .assets
        .iter()
        .find(|asset| asset.kind == ReleaseAssetKind::ServerBundle)
        .context("Release has no server bundle containing a Windows worker kit")?;
    let cli = asset_for_current_platform(&manifest.assets, ReleaseAssetKind::Cli)?;
    ensure!(
        bundle.sha256.is_some() && cli.sha256.is_some(),
        "The worker kit requires bundle and CLI checksums"
    );
    let checksum = bundle.sha256.as_deref().unwrap();
    ensure!(
        checksum.len() == 64 && checksum.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid worker bundle checksum"
    );
    let release = root.join(format!("release.{revision}.{checksum}"));
    if !release.exists() {
        stage(root, &release, revision, bundle, cli, locator)?;
    } else {
        ensure!(
            release.canonicalize()? == release
                && fs::read_to_string(release.join("REVISION"))?.trim() == revision,
            "Installed worker revision changed"
        );
        verify_asset(cli, &release.join("bin/gah"))?;
        probe_downloaded_binary(&release.join("bin/gah"))?;
        probe_runtime(&release)?;
    }
    let binary = release.join("bin/gah");
    let node = release.join("bin/node");
    let config_home = crate::update::user_config_home()?;
    let selected = crate::update::agents_to_refresh(&config_home, agents);
    crate::update::install_selected_agent_assets(&release, &config_home, &selected)?;
    let worker_env = root.join("worker.env");
    let runtime_env = root.join("release.env");
    let hook = format!("source {}", shell_quote(&runtime_env.to_string_lossy()));
    let mut worker_settings = fs::read_to_string(&worker_env)?;
    let migrate = !worker_settings.lines().any(|line| line == hook);
    if migrate {
        crate::installer::wsl::write_launchers(root, central)?;
        worker_settings.push_str(&format!("\n{hook}\n"));
    }
    let runtime = format!(
        "export PARASAUR_WORKER_RELEASE={checksum}\nexport PARASAUR_BINARY={}\nexport PARASAUR_WORKER_NODE={}\nexport PATH={}:\"$PATH\"\n",
        shell_quote(&binary.to_string_lossy()),
        shell_quote(&node.to_string_lossy()),
        shell_quote(&release.join("bin").to_string_lossy())
    );
    replace(&runtime_env, runtime.as_bytes(), 0o600)?;
    if migrate {
        replace(&worker_env, worker_settings.as_bytes(), 0o600)?;
    }
    fs::File::open(root)?.sync_all()?;
    println!(
        "Installed Windows worker release {revision} in {}. Restart gah-worker.service to load it.",
        release.display()
    );
    Ok(())
}

fn stage(
    root: &Path,
    release: &Path,
    revision: &str,
    bundle: &ReleaseAsset,
    cli: &ReleaseAsset,
    locator: &AssetLocator,
) -> Result<()> {
    let download = tempfile::tempdir_in(root)?;
    let archive = download.path().join("bundle.tar.gz");
    fetch_asset(locator, bundle, &archive)?;
    verify_asset(bundle, &archive)?;
    let unpack = Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(download.path())
        .args([format!("{KIT}/gah"), format!("{KIT}/source.tar.gz")])
        .status()?;
    ensure!(
        unpack.success(),
        "This server release has no complete Windows worker kit"
    );
    let kit = download.path().join(KIT);
    verify_asset(cli, &kit.join("gah"))?;
    let staged = tempfile::tempdir_in(root)?;
    let dir = staged.path();
    let unpack = Command::new("tar")
        .arg("-xzf")
        .arg(kit.join("source.tar.gz"))
        .arg("-C")
        .arg(dir)
        .status()?;
    ensure!(
        unpack.success()
            && fs::read_to_string(dir.join("REVISION"))
                .unwrap_or_default()
                .trim()
                == revision,
        "Expected a prebuilt Windows worker kit from release {revision}"
    );
    let binary = dir.join("bin/gah");
    fs::rename(kit.join("gah"), &binary)?;
    probe_downloaded_binary(&binary)?;
    probe_runtime(dir)?;
    fs::rename(dir, release)?;
    Ok(())
}

fn probe_runtime(dir: &Path) -> Result<()> {
    super::linux_runtime::probe(&dir.join("bin/node"), &dir.join("apps/server/dist"))
}
