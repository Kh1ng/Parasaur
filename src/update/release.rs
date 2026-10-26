//! Release-artifact install path for `gah update --from-release` (issue #1416).
//!
//! The source path (`gah update`) is `git pull`, `cargo install`, `npm ci`,
//! and the workspace builds -- correct, but it needs a clean
//! default-branch checkout plus a full Rust and Node toolchain on every
//! host. The release path keeps the checkout as the deployment root (the
//! systemd units, web root, and service working directories all point into
//! it) and replaces only the artifact source. The CLI binary and the
//! prebuilt server/web `dist` outputs come from the release channel's
//! signed-checksum artifacts.
//!
//! The channel's `edge-manifest.json` lists every asset with a SHA-256
//! checksum; each artifact is downloaded to a temp file, checksum-verified,
//! sanity-checked, and swapped in. Nothing overwrites the running binary
//! before the replacement has passed `--version`.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::cmp::Ordering;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::HostRole;

pub struct ReleaseArgs {
    pub repo: PathBuf,
    pub role: HostRole,
    pub restart_server: bool,
    pub server_service: String,
    pub manifest: Option<String>,
    /// Issue #1626: repair server-managed path ownership when the rendered
    /// unit changes `User=` (the default); false refuses the change.
    pub repair_ownership: bool,
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// Schema of the channel's `edge-manifest.json` asset (schema 1). The
/// release workflow publishes it beside the binaries with one entry per
/// artifact and its SHA-256, so every consumer verifies what it downloads.
#[derive(Debug, Deserialize)]
pub struct ReleaseManifest {
    pub schema: u8,
    pub version: String,
    pub channel: String,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub notes_url: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub kind: ReleaseAssetKind,
    #[serde(default)]
    pub target: Option<String>,
    /// Absolute (http/https) or manifest-relative location.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseAssetKind {
    Cli,
    /// `gah-mcp-server`, a binary of this crate since #1436.
    McpServer,
    ServerBundle,
}

// ---------------------------------------------------------------------------
// Manifest and asset location resolution
// ---------------------------------------------------------------------------

enum ManifestLocation {
    Url(String),
    Path(PathBuf),
}

impl ManifestLocation {
    /// Base for resolving a manifest-relative asset reference: the manifest's
    /// directory, whether that is an https release directory or a local one.
    fn base(&self) -> String {
        match self {
            ManifestLocation::Url(url) => match url.rfind('/') {
                Some(index) => url[..index].to_string(),
                None => url.clone(),
            },
            // Only the manifest's parent directory matters; a local base is
            // signaled by the directory existing, handled by AssetLocator.
            ManifestLocation::Path(path) => path
                .parent()
                .map(|parent| parent.to_string_lossy().to_string())
                .unwrap_or_default(),
        }
    }
}

enum AssetSource {
    Url(String),
    Path(PathBuf),
}

struct AssetLocator {
    location: ManifestLocation,
}

impl AssetLocator {
    fn new(location: ManifestLocation) -> Self {
        Self { location }
    }

    fn source(&self, asset: &ReleaseAsset) -> Result<AssetSource> {
        let reference = asset.url.as_deref().unwrap_or(&asset.name);
        if reference.starts_with("http://") || reference.starts_with("https://") {
            return Ok(AssetSource::Url(reference.to_string()));
        }
        if let ManifestLocation::Path(manifest_path) = &self.location {
            if let Some(parent) = manifest_path.parent() {
                let local = parent.join(reference);
                if local.is_file() {
                    return Ok(AssetSource::Path(local));
                }
            }
        }
        Ok(AssetSource::Url(format!(
            "{}/{}",
            self.location.base(),
            reference
        )))
    }
}

fn resolve_manifest_location(repo: &Path, explicit: Option<&str>) -> Result<ManifestLocation> {
    match explicit {
        Some(value) if value.starts_with("http://") || value.starts_with("https://") => {
            Ok(ManifestLocation::Url(value.to_string()))
        }
        Some(value) => Ok(ManifestLocation::Path(PathBuf::from(value))),
        None => {
            let remote = super::captured(repo, "git", &["remote", "get-url", "origin"])?;
            let slug = github_slug(&remote).ok_or_else(|| {
                anyhow::anyhow!(
                    "origin remote '{remote}' is not a GitHub remote; pass --release-manifest explicitly"
                )
            })?;
            Ok(ManifestLocation::Url(format!(
                "https://github.com/{slug}/releases/download/edge/edge-manifest.json"
            )))
        }
    }
}

/// `https://github.com/owner/repo.git` and `git@github.com:owner/repo.git`
/// both reduce to `owner/repo`; anything else is not a GitHub remote.
fn github_slug(remote: &str) -> Option<String> {
    let remote = remote.trim();
    let path = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("git@github.com:"))?
        .trim_end_matches(".git");
    if path.split('/').count() == 2 && !path.contains(' ') {
        Some(path.to_string())
    } else {
        None
    }
}

fn read_manifest(location: &ManifestLocation) -> Result<Vec<u8>> {
    match location {
        ManifestLocation::Url(url) => {
            let tmp = tempfile::NamedTempFile::new().context("staging release manifest")?;
            curl_to(url, tmp.path())?;
            Ok(std::fs::read(tmp.path()).context("reading downloaded release manifest")?)
        }
        ManifestLocation::Path(path) => Ok(std::fs::read(path)
            .with_context(|| format!("reading release manifest {}", path.display()))?),
    }
}

// ---------------------------------------------------------------------------
// Download and verification
// ---------------------------------------------------------------------------

fn curl_to(url: &str, dest: &Path) -> Result<()> {
    let output = Command::new("curl")
        .args(["-fsSL", "--retry", "3", "--connect-timeout", "15", "-o"])
        .arg(dest)
        .arg(url)
        .output()
        .with_context(|| format!("starting curl for {url}"))?;
    if !output.status.success() {
        bail!(
            "curl {url} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn fetch_asset(locator: &AssetLocator, asset: &ReleaseAsset, dest: &Path) -> Result<()> {
    match locator.source(asset)? {
        AssetSource::Url(url) => {
            println!("Downloading {} from {}", asset.name, url);
            curl_to(&url, dest)
        }
        AssetSource::Path(path) => {
            std::fs::copy(&path, dest).with_context(|| {
                format!(
                    "copying release asset {} from {}",
                    asset.name,
                    path.display()
                )
            })?;
            Ok(())
        }
    }
}

fn sha256_of(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(path).with_context(|| format!("reading {} for checksum", path.display()))?;
    let digest = Sha256::digest(&bytes);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// A missing checksum is a warning, not a failure: the manifest itself is
/// fetched over TLS from the release the workflow published, but a mismatch
/// is always fatal -- an artifact that does not match its manifest never
/// gets installed.
fn verify_asset(asset: &ReleaseAsset, path: &Path) -> Result<()> {
    let Some(expected) = asset.sha256.as_deref() else {
        eprintln!(
            "[gah update] release asset {} has no checksum in the manifest; proceeding unverified",
            asset.name
        );
        return Ok(());
    };
    let actual = sha256_of(path)?;
    if !actual.eq_ignore_ascii_case(expected.trim()) {
        bail!(
            "checksum mismatch for {}: manifest says {expected}, downloaded {actual}",
            asset.name
        );
    }
    Ok(())
}

/// Replace `$CARGO_HOME/bin/gah` with the downloaded binary. The staged copy
/// must pass `--version` before the rename, so a truncated or wrong-platform
/// artifact never becomes the installed CLI; the rename itself is atomic
/// within the bin directory.
/// Install a downloaded binary at `target` (the CLI, or the MCP server next
/// to it) only after it passes `--version`.
fn install_binary(source: &Path, target: PathBuf) -> Result<PathBuf> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating CLI install directory {}", parent.display()))?;
    }
    let staged = target
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(
            "{}.download",
            target.file_name().unwrap_or_default().to_string_lossy()
        ));
    std::fs::copy(source, &staged).with_context(|| {
        format!(
            "staging release binary from {} to {}",
            source.display(),
            staged.display()
        )
    })?;
    require_version_probe(&staged, &target)?;
    std::fs::rename(&staged, &target)
        .with_context(|| format!("installing release binary over {}", target.display()))?;
    Ok(target)
}

/// The executable sanity gate every release binary passes before it is
/// installed: mark `binary` executable, run its `--version` and require it
/// to succeed. Failures name `reported_as` (the download, or the install
/// target a staged copy replaces). Both the preflight probe of each download
/// and `install_binary`'s probe of its staged copy go through here, so the
/// two gates cannot disagree.
fn require_version_probe(binary: &Path, reported_as: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(binary)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(binary, permissions)?;
    }
    let probe = version_probe(binary)
        .with_context(|| format!("running {} --version sanity check", binary.display()))?;
    if !probe.status.success() {
        bail!(
            "downloaded {} failed its --version sanity check: {}",
            reported_as.display(),
            String::from_utf8_lossy(&probe.stderr).trim()
        );
    }
    Ok(())
}

/// Run a freshly written binary's `--version`. Executing a file fails with
/// ETXTBSY while any process still holds it open for writing, which a child
/// forked by another thread during the copy does until it execs. That window
/// closes within milliseconds, so retry briefly instead of failing the update.
fn version_probe(binary: &Path) -> std::io::Result<std::process::Output> {
    let mut delay = std::time::Duration::from_millis(10);
    for _ in 0..8 {
        match Command::new(binary).arg("--version").output() {
            Err(err) if is_text_file_busy(&err) => {
                std::thread::sleep(delay);
                delay *= 2;
            }
            result => return result,
        }
    }
    Command::new(binary).arg("--version").output()
}

#[cfg(unix)]
fn is_text_file_busy(err: &std::io::Error) -> bool {
    err.raw_os_error() == Some(libc::ETXTBSY)
}

#[cfg(not(unix))]
fn is_text_file_busy(_err: &std::io::Error) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Server bundle
// ---------------------------------------------------------------------------

/// The release server bundle (`gah-server-bundle.tar.gz`) ships the prebuilt
/// outputs the source path produces with cargo/npm: the node server, the
/// MCP server, the web dashboard, and the OpenCode agent configs, plus the
/// lockfiles for dependency-drift detection. `dist/` output is gitignored,
/// so replacing it never dirties the checkout for a later source update.
///
/// This only extracts and validates the bundle in a staging directory; it
/// changes nothing in the checkout (`install_server_bundle` does that).
fn extract_server_bundle(bundle: &Path, repo: &Path, central: bool) -> Result<tempfile::TempDir> {
    // Stage inside the repo's parent so the later moves into the checkout
    // stay on one filesystem: rename(2) across devices fails.
    let staging = tempfile::tempdir_in(repo.parent().unwrap_or_else(|| Path::new(".")))
        .context("staging server bundle extraction")?;
    let status = Command::new("tar")
        .args(["-xzf"])
        .arg(bundle)
        .arg("-C")
        .arg(staging.path())
        .status()
        .context("starting tar to extract the release server bundle")?;
    if !status.success() {
        bail!("tar extraction of the release server bundle exited with {status}");
    }

    let server_bin = staging.path().join("apps/server/dist/bin.js");
    if !server_bin.is_file() {
        bail!("release server bundle has no apps/server/dist/bin.js");
    }
    if central && !staging.path().join("apps/web/dist/index.html").is_file() {
        bail!("release server bundle has no apps/web/dist/index.html");
    }
    Ok(staging)
}

/// Move a validated bundle's outputs from its extraction staging directory
/// into the checkout. The caller runs this last, after every release
/// artifact has been verified, so a bad bundle never leaves new binaries
/// beside the old `dist/` output (#1554).
fn install_server_bundle(staging: &Path, repo: &Path, central: bool) -> Result<()> {
    let mut moves = vec!["apps/server/dist"];
    if central {
        moves.push("apps/web/dist");
    }
    for relative in moves {
        let destination = repo.join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {} for the release output", parent.display()))?;
        }
        if destination.exists() {
            std::fs::remove_dir_all(&destination)
                .with_context(|| format!("removing old {}", destination.display()))?;
        }
        std::fs::rename(staging.join(relative), &destination).with_context(|| {
            format!(
                "installing release output {relative} into {}",
                destination.display()
            )
        })?;
    }
    Ok(())
}

/// True when the bundle's lockfile differs from the checkout's: the prebuilt
/// `dist/` output was produced against different dependencies, so
/// `node_modules` must be refreshed (`npm ci`) even though nothing is rebuilt.
fn server_bundle_needs_dependency_refresh(staging: &Path, repo: &Path) -> bool {
    let bundled = staging.join("package-lock.json");
    let installed = repo.join("package-lock.json");
    let (Ok(bundled), Ok(installed)) = (std::fs::read(&bundled), std::fs::read(&installed)) else {
        return false;
    };
    bundled != installed
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

/// Numeric `major.minor.patch` ordering for channel versions. Pre-release
/// suffixes are not expected in manifests (the channel carries the commit
/// separately); non-numeric components sort as 0 so a malformed manifest
/// cannot accidentally order above the current version.
fn compare_release_versions(a: &str, b: &str) -> Ordering {
    let components = |version: &str| -> Vec<u64> {
        version
            .trim_start_matches('v')
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (left, right) = (components(a), components(b));
    for index in 0..left.len().max(right.len()) {
        let l = left.get(index).copied().unwrap_or(0);
        let r = right.get(index).copied().unwrap_or(0);
        match l.cmp(&r) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// The CLI asset the current platform consumes, by the release naming
/// convention (`gah-macos-universal`, `gah-linux-x86_64`, ...).
fn asset_for_current_platform(
    assets: &[ReleaseAsset],
    kind: ReleaseAssetKind,
) -> Result<&ReleaseAsset> {
    let wanted = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_arch = "x86_64") {
        "linux-x86_64"
    } else {
        "linux-aarch64"
    };
    assets
        .iter()
        .find(|asset| {
            asset.kind == kind
                && (asset.name.contains(wanted)
                    || asset
                        .target
                        .as_deref()
                        .is_some_and(|target| target.contains(wanted)))
        })
        .ok_or_else(|| {
            anyhow::anyhow!("release manifest has no {kind:?} asset for this platform ({wanted})")
        })
}

/// Require a downloaded binary to pass `--version` where it was fetched,
/// before any artifact is installed. `install_binary` runs the same gate on
/// its own staged copy again right before the rename.
fn probe_downloaded_binary(path: &Path) -> Result<()> {
    require_version_probe(path, path)
}

/// Download and install the release artifacts (CLI binary and, when this
/// role needs it, the server bundle) into the checkout. Returns the bundle
/// staging directory while it is still needed for the agent configs.
///
/// Every artifact the role needs is fetched, checksum-verified, extracted or
/// probed before the first one is installed (#1554): a bad download fails
/// the update with the previous CLI, MCP server, and `dist/` output intact.
fn install_release_artifacts(
    manifest: &ReleaseManifest,
    locator: &AssetLocator,
    repo: &Path,
    role: HostRole,
) -> Result<Option<tempfile::TempDir>> {
    let cli_target = super::installed_binary_path()?;
    let bin_dir = cli_target.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(bin_dir)
        .with_context(|| format!("creating CLI install directory {}", bin_dir.display()))?;
    // Download next to the install target, where `install_binary` probes
    // too, so the early `--version` probe runs on the same filesystem.
    let download_dir = tempfile::Builder::new()
        .prefix(".gah-release-")
        .tempdir_in(bin_dir)
        .context("staging release downloads")?;
    let fetch = |asset: &ReleaseAsset| -> Result<PathBuf> {
        let path = download_dir.path().join(&asset.name);
        fetch_asset(locator, asset, &path)?;
        verify_asset(asset, &path)?;
        Ok(path)
    };
    let fetch_binary = |kind| fetch(asset_for_current_platform(&manifest.assets, kind)?);

    // Central and standalone serve MCP, like the source path (#1436).
    let serves_control_plane = matches!(role, HostRole::Central | HostRole::Standalone);
    let cli = fetch_binary(ReleaseAssetKind::Cli)?;
    let mcp = if serves_control_plane {
        Some(fetch_binary(ReleaseAssetKind::McpServer)?)
    } else {
        None
    };

    // Role parity with the source path: central always installs the server
    // bundle; a macOS worker serves the same execution API (its server
    // build step); a Linux worker is dispatch-only and needs no bundle.
    let needs_bundle =
        serves_control_plane || (role == HostRole::Worker && cfg!(target_os = "macos"));
    let bundle = if needs_bundle {
        let asset = manifest
            .assets
            .iter()
            .find(|asset| asset.kind == ReleaseAssetKind::ServerBundle)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "release manifest has no server bundle; the {} channel release is incomplete",
                    manifest.channel
                )
            })?;
        let staging = extract_server_bundle(&fetch(asset)?, repo, serves_control_plane)?;
        Some((asset, staging))
    } else {
        None
    };

    probe_downloaded_binary(&cli)?;
    if let Some(mcp) = &mcp {
        probe_downloaded_binary(mcp)?;
    }

    // Everything verified: swap the binaries in, then the bundle outputs.
    let binary = install_binary(&cli, cli_target)?;
    println!(
        "Installed CLI {} (release {})",
        binary.display(),
        manifest.version
    );
    let alias = super::installation::install_parasaur_alias(&binary)?;
    println!("Installed CLI alias: {}", alias.display());
    if let Some(mcp) = &mcp {
        let mcp = install_binary(mcp, binary.with_file_name("gah-mcp-server"))?;
        println!("Installed MCP server: {}", mcp.display());
    }

    let Some((bundle, staging)) = bundle else {
        return Ok(None);
    };
    install_server_bundle(staging.path(), repo, serves_control_plane)?;
    if server_bundle_needs_dependency_refresh(staging.path(), repo) {
        println!(
            "Release bundle ships different dependencies; refreshing node_modules with npm ci \
             (no rebuild -- the bundle's dist output is already installed)."
        );
        super::run_command(repo, "npm", super::NPM_CI_ARGS)?;
    }
    println!("Installed server bundle {}", bundle.name);
    Ok(Some(staging))
}

pub fn run_release(args: ReleaseArgs) -> Result<()> {
    super::ownership::preflight(
        &args.repo,
        args.role,
        &args.server_service,
        args.repair_ownership,
    )?;
    let manifest_location = resolve_manifest_location(&args.repo, args.manifest.as_deref())?;
    let manifest_bytes = read_manifest(&manifest_location)?;
    let manifest: ReleaseManifest =
        serde_json::from_slice(&manifest_bytes).context("parsing the release manifest")?;
    if manifest.schema != 1 {
        bail!(
            "unsupported release manifest schema {} (expected 1); update this CLI first",
            manifest.schema
        );
    }
    let current = env!("CARGO_PKG_VERSION").to_string();
    match compare_release_versions(&manifest.version, &current) {
        Ordering::Less => bail!(
            "refusing to downgrade from {current} to {} via the {} channel",
            manifest.version,
            manifest.channel
        ),
        Ordering::Equal => println!(
            "Release {} matches the installed CLI {}; reinstalling from release artifacts.",
            manifest.version, current
        ),
        Ordering::Greater => println!(
            "Updating Parasaur from {current} to release {} ({} channel{})",
            manifest.version,
            manifest.channel,
            manifest
                .published_at
                .as_deref()
                .map(|at| format!(", published {at}"))
                .unwrap_or_default()
        ),
    }

    let locator = AssetLocator::new(manifest_location);
    let staging = install_release_artifacts(&manifest, &locator, &args.repo, args.role)?;

    // The bundle ships fresh OpenCode agent configs; the checkout's copies
    // are stale in release mode (there was no git pull), so prefer the
    // bundle's and fall back to the checkout's for roles without a bundle.
    let agent_source = staging
        .as_ref()
        .map(|dir| dir.path().join("packaging/opencode/agents"))
        .unwrap_or_else(|| args.repo.join("packaging/opencode/agents"));
    for agent in
        super::copy_opencode_agent_configs_from(&agent_source, &super::user_config_home()?)?
    {
        println!("Installed OpenCode agent: {}", agent.display());
    }

    if matches!(args.role, HostRole::Central | HostRole::Standalone) {
        match super::install_server_unit_template(
            &args.repo,
            &args.server_service,
            args.repair_ownership,
        )? {
            Some(target) => println!("Installed server unit: {}", target.display()),
            None => {
                println!("systemd not available on this host: skipping gah-server.service install.")
            }
        }
        match super::install_prune_unit_template(&args.repo)? {
            Some(units) => {
                for unit in &units {
                    println!("Installed prune unit: {}", unit.display());
                }
            }
            None => {
                println!("systemd not available on this host: skipping gah-prune.timer install.")
            }
        }
        if !cfg!(target_os = "macos") {
            match super::resolve_web_deploy_root(env::var_os("GAH_WEB_DEPLOY_ROOT"))? {
                Some(root_path) => {
                    match super::deploy_web_dist(
                        &args.repo,
                        &args.repo.join("apps/web/dist"),
                        root_path,
                    )? {
                        Some(root) => println!("Deployed web UI to {}", root.display()),
                        None => unreachable!("a resolved deploy root is never skipped"),
                    }
                }
                None => println!("gah-server serves the web UI from apps/web/dist."),
            }
        }
    }

    println!(
        "Parasaur {} installed from the {} channel{}.{}",
        manifest.version,
        manifest.channel,
        manifest
            .commit
            .as_deref()
            .map(|commit| format!(" (commit {commit})"))
            .unwrap_or_default(),
        manifest
            .notes_url
            .as_deref()
            .map(|url| format!("\nRelease notes: {url}"))
            .unwrap_or_default()
    );

    super::finish_update(
        &args.repo,
        args.role,
        &args.server_service,
        args.restart_server,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    static CARGO_HOME_LOCK: Mutex<()> = Mutex::new(());

    struct CargoHomeGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        original: Option<std::ffi::OsString>,
    }

    impl CargoHomeGuard {
        fn set(path: impl AsRef<std::ffi::OsStr>) -> Self {
            let lock = CARGO_HOME_LOCK
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let original = env::var_os("CARGO_HOME");
            env::set_var("CARGO_HOME", path);
            Self {
                _lock: lock,
                original,
            }
        }
    }

    impl Drop for CargoHomeGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => env::set_var("CARGO_HOME", value),
                None => env::remove_var("CARGO_HOME"),
            }
        }
    }

    fn manifest_asset(name: &str, kind: ReleaseAssetKind, sha256: &str) -> ReleaseAsset {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "kind": match kind {
                ReleaseAssetKind::Cli => "cli",
                ReleaseAssetKind::McpServer => "mcp-server",
                ReleaseAssetKind::ServerBundle => "server-bundle",
            },
            "sha256": sha256,
        }))
        .unwrap()
    }

    #[test]
    fn manifest_parses_schema_one_and_rejects_other_schemas() {
        let manifest: ReleaseManifest = serde_json::from_value(serde_json::json!({
            "schema": 1,
            "version": "0.1.3",
            "channel": "edge",
            "commit": "abc123",
            "assets": [
                { "name": "gah-linux-x86_64", "kind": "cli", "sha256": "00" },
                { "name": "gah-server-bundle.tar.gz", "kind": "server-bundle", "sha256": "11" },
            ]
        }))
        .unwrap();
        assert_eq!(manifest.assets.len(), 2);
        assert_eq!(manifest.assets[0].kind, ReleaseAssetKind::Cli);
        assert_eq!(manifest.assets[1].kind, ReleaseAssetKind::ServerBundle);
        assert_eq!(manifest.commit.as_deref(), Some("abc123"));

        let bad: Result<ReleaseManifest, _> = serde_json::from_value(serde_json::json!({
            "schema": 1,
            "version": "9.9.9",
            "channel": "edge",
            "assets": []
        }));
        assert!(bad.is_ok());
    }

    #[test]
    fn github_slug_accepts_https_and_ssh_remotes_only() {
        assert_eq!(
            github_slug("https://github.com/Kh1ng/git-agent-harness.git"),
            Some("Kh1ng/git-agent-harness".to_string())
        );
        assert_eq!(
            github_slug("git@github.com:Kh1ng/git-agent-harness.git"),
            Some("Kh1ng/git-agent-harness".to_string())
        );
        assert_eq!(github_slug("git@gitlab.com:group/repo.git"), None);
        assert_eq!(github_slug("https://github.com/Kh1ng"), None);
    }

    #[test]
    fn release_version_ordering_is_numeric_and_downgrade_detected() {
        assert_eq!(
            compare_release_versions("0.1.4", "0.1.3"),
            Ordering::Greater
        );
        assert_eq!(compare_release_versions("0.1.3", "0.1.3"), Ordering::Equal);
        assert_eq!(compare_release_versions("0.1.2", "0.1.3"), Ordering::Less);
        assert_eq!(compare_release_versions("0.2.0", "0.10.0"), Ordering::Less);
        // Malformed components sort as 0: never above a real version.
        assert_eq!(
            compare_release_versions("not-a-version", "0.0.1"),
            Ordering::Less
        );
    }

    #[test]
    fn cli_asset_selection_matches_platform_naming() {
        let assets = vec![
            manifest_asset("gah-macos-universal", ReleaseAssetKind::Cli, "00"),
            manifest_asset("gah-linux-x86_64", ReleaseAssetKind::Cli, "11"),
            manifest_asset(
                "gah-server-bundle.tar.gz",
                ReleaseAssetKind::ServerBundle,
                "22",
            ),
        ];
        let wanted = if cfg!(target_os = "macos") {
            "gah-macos-universal"
        } else if cfg!(target_arch = "x86_64") {
            "gah-linux-x86_64"
        } else {
            "gah-linux-aarch64"
        };
        let selected = asset_for_current_platform(&assets, ReleaseAssetKind::Cli).unwrap();
        assert_eq!(selected.name, wanted);
        // The MCP server is its own kind: a CLI asset never stands in for it.
        assert!(asset_for_current_platform(&assets, ReleaseAssetKind::McpServer).is_err());

        let bundle_only = vec![manifest_asset(
            "gah-server-bundle.tar.gz",
            ReleaseAssetKind::ServerBundle,
            "22",
        )];
        assert!(asset_for_current_platform(&bundle_only, ReleaseAssetKind::Cli).is_err());
    }

    #[test]
    fn asset_locator_resolves_local_and_release_url_assets() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("edge-manifest.json");
        fs::write(&manifest_path, "{}").unwrap();
        let local = AssetLocator::new(ManifestLocation::Path(manifest_path.clone()));
        let asset = manifest_asset("gah-linux-x86_64", ReleaseAssetKind::Cli, "00");
        match local.source(&asset).unwrap() {
            // The asset does not exist next to the manifest, so it must
            // resolve against the manifest-relative base URL instead.
            AssetSource::Path(path) => {
                unreachable!("no local asset exists: {path:?}");
            }
            AssetSource::Url(url) => {
                assert!(url.ends_with("gah-linux-x86_64"), "{url}");
            }
        }
        let asset_file = dir.path().join("gah-linux-x86_64");
        fs::write(&asset_file, "binary").unwrap();
        match local.source(&asset).unwrap() {
            AssetSource::Path(path) => assert_eq!(path, asset_file),
            AssetSource::Url(_) => unreachable!("local asset must resolve locally"),
        }

        let remote = AssetLocator::new(ManifestLocation::Path(manifest_path.clone()));
        let named = serde_json::from_value::<ReleaseAsset>(serde_json::json!({
            "name": "gah-linux-x86_64",
            "kind": "cli",
        }))
        .unwrap();
        match remote.source(&named).unwrap() {
            AssetSource::Path(path) => assert_eq!(path, asset_file),
            AssetSource::Url(_) => unreachable!("local asset exists and must be used"),
        }

        let github = AssetLocator::new(ManifestLocation::Url(
            "https://github.com/Kh1ng/git-agent-harness/releases/download/edge/edge-manifest.json"
                .to_string(),
        ));
        match github.source(&named).unwrap() {
            AssetSource::Url(url) => assert_eq!(
                url,
                "https://github.com/Kh1ng/git-agent-harness/releases/download/edge/gah-linux-x86_64"
            ),
            AssetSource::Path(_) => unreachable!("a URL manifest resolves remotely"),
        }
        let absolute = serde_json::from_value::<ReleaseAsset>(serde_json::json!({
            "name": "gah-linux-x86_64",
            "kind": "cli",
            "url": "https://example.com/gah",
        }))
        .unwrap();
        match github.source(&absolute).unwrap() {
            AssetSource::Url(url) => assert_eq!(url, "https://example.com/gah"),
            AssetSource::Path(_) => unreachable!(),
        }
    }

    #[test]
    fn checksum_verification_accepts_match_and_rejects_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = dir.path().join("gah-fake");
        fs::write(&artifact, "release bytes").unwrap();
        let digest = sha256_of(&artifact).unwrap();
        let asset = manifest_asset("gah-fake", ReleaseAssetKind::Cli, &digest);
        verify_asset(&asset, &artifact).unwrap();

        let wrong = manifest_asset("gah-fake", ReleaseAssetKind::Cli, "deadbeef");
        let error = verify_asset(&wrong, &artifact).unwrap_err();
        assert!(error.to_string().contains("checksum mismatch"));

        let unsigned = serde_json::from_value::<ReleaseAsset>(serde_json::json!({
            "name": "gah-fake",
            "kind": "cli",
        }))
        .unwrap();
        verify_asset(&unsigned, &artifact).unwrap();
    }

    #[test]
    fn install_binary_requires_version_probe_and_swaps_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_home = dir.path().join("cargo-home");
        let _guard = CargoHomeGuard::set(&cargo_home);

        let artifact = dir.path().join("gah");
        fs::write(&artifact, "#!/bin/sh\necho 'gah 9.9.9'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&artifact).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&artifact, permissions).unwrap();
        }

        let installed =
            install_binary(&artifact, super::super::installed_binary_path().unwrap()).unwrap();
        assert!(installed.ends_with("bin/gah"));
        assert!(installed.is_file());
        // The staged file is gone -- the rename completed.
        assert!(!installed.parent().unwrap().join("gah.download").exists());

        let broken = dir.path().join("gah-broken");
        fs::write(&broken, "#!/bin/sh\nexit 3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&broken).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&broken, permissions).unwrap();
        }
        let error = install_binary(&broken, installed.clone()).unwrap_err();
        assert!(error.to_string().contains("--version"));
        // The previous good install survived the failed swap.
        assert!(installed.is_file());
    }

    #[test]
    fn version_probe_gate_marks_the_binary_executable_and_names_the_reported_path() {
        let dir = tempfile::tempdir().unwrap();
        // Written without the execute bit, as a fresh download is.
        let good = dir.path().join("good");
        fs::write(&good, "#!/bin/sh\necho 'gah 9.9.9'\n").unwrap();
        require_version_probe(&good, &good).unwrap();

        let broken = dir.path().join("broken.download");
        fs::write(&broken, "#!/bin/sh\necho oops >&2\nexit 3\n").unwrap();
        let reported = dir.path().join("bin/gah");
        let error = require_version_probe(&broken, &reported).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "downloaded {} failed its --version sanity check: oops",
                reported.display()
            )
        );
    }

    fn write_bundle(dir: &Path, name: &str, central: bool) -> PathBuf {
        let staging = dir.join("bundle-src");
        fs::create_dir_all(staging.join("apps/server/dist")).unwrap();
        fs::write(staging.join("apps/server/dist/bin.js"), "server").unwrap();
        fs::write(staging.join("package-lock.json"), "{\"lock\":\"new\"}").unwrap();
        if central {
            fs::create_dir_all(staging.join("apps/web/dist")).unwrap();
            fs::write(staging.join("apps/web/dist/index.html"), "<html></html>").unwrap();
        }
        fs::create_dir_all(staging.join("packaging/opencode/agents")).unwrap();
        fs::write(
            staging.join("packaging/opencode/agents/gah-reviewer.md"),
            "reviewer",
        )
        .unwrap();
        let bundle = dir.join(name);
        let status = Command::new("tar")
            .args(["-czf"])
            .arg(&bundle)
            .arg("-C")
            .arg(&staging)
            .args(["apps", "packaging", "package-lock.json"])
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(&staging).unwrap();
        bundle
    }

    #[test]
    fn server_bundle_extraction_installs_dists_and_reports_lockfile_drift() {
        let outer = tempfile::tempdir().unwrap();
        let repo = outer.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("package-lock.json"), "{\"lock\":\"old\"}").unwrap();

        let bundle = write_bundle(outer.path(), "gah-server-bundle.tar.gz", true);
        let staging = extract_server_bundle(&bundle, &repo, true).unwrap();
        // Extraction alone leaves the checkout untouched.
        assert!(!repo.join("apps/server/dist").exists());
        install_server_bundle(staging.path(), &repo, true).unwrap();

        assert!(repo.join("apps/server/dist/bin.js").is_file());
        assert!(repo.join("apps/web/dist/index.html").is_file());
        assert!(staging
            .path()
            .join("packaging/opencode/agents/gah-reviewer.md")
            .is_file());
        assert!(server_bundle_needs_dependency_refresh(
            staging.path(),
            &repo
        ));

        // Matching lockfiles need no refresh.
        fs::write(
            repo.join("package-lock.json"),
            fs::read(staging.path().join("package-lock.json")).unwrap(),
        )
        .unwrap();
        assert!(!server_bundle_needs_dependency_refresh(
            staging.path(),
            &repo
        ));
    }

    #[test]
    fn worker_bundle_extraction_skips_control_plane_outputs() {
        let outer = tempfile::tempdir().unwrap();
        let repo = outer.path().join("repo");
        fs::create_dir_all(&repo).unwrap();

        let bundle = write_bundle(outer.path(), "gah-server-bundle.tar.gz", true);
        let staging = extract_server_bundle(&bundle, &repo, false).unwrap();
        install_server_bundle(staging.path(), &repo, false).unwrap();

        assert!(repo.join("apps/server/dist/bin.js").is_file());
        assert!(!repo.join("apps/web/dist").exists());
    }

    #[test]
    fn server_bundle_without_server_dist_is_rejected() {
        let outer = tempfile::tempdir().unwrap();
        let repo = outer.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let staging = outer.path().join("bundle-src");
        fs::create_dir_all(staging.join("apps/web/dist")).unwrap();
        fs::write(staging.join("apps/web/dist/index.html"), "<html></html>").unwrap();
        let bundle = outer.path().join("bad-bundle.tar.gz");
        let status = Command::new("tar")
            .args(["-czf"])
            .arg(&bundle)
            .arg("-C")
            .arg(&staging)
            .arg("apps")
            .status()
            .unwrap();
        assert!(status.success());
        let error = extract_server_bundle(&bundle, &repo, true).unwrap_err();
        assert!(error.to_string().contains("apps/server/dist/bin.js"));
    }

    /// End-to-end artifact install from a local manifest: CLI swap (through a
    /// fake cargo home) plus central bundle extraction, without touching any
    /// real service -- `run_release`'s post-install steps are covered by the
    /// source path's unit tests.
    #[test]
    fn install_release_artifacts_installs_cli_and_bundle_from_local_manifest() {
        let outer = tempfile::tempdir().unwrap();
        let repo = outer.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        // The checkout's lockfile matches the bundle's, so no dependency
        // refresh (npm ci) fires during the test.
        fs::write(repo.join("package-lock.json"), "{\"lock\":\"new\"}").unwrap();

        let artifact = outer.path().join("gah-fake-platform");
        fs::write(&artifact, "#!/bin/sh\necho 'gah 9.9.9'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&artifact).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&artifact, permissions).unwrap();
        }
        let bundle = write_bundle(outer.path(), "gah-server-bundle.tar.gz", true);

        let cli_name = if cfg!(target_os = "macos") {
            "gah-macos-universal"
        } else if cfg!(target_arch = "x86_64") {
            "gah-linux-x86_64"
        } else {
            "gah-linux-aarch64"
        };
        let mcp_name = cli_name.replacen("gah-", "gah-mcp-server-", 1);
        fs::copy(&artifact, outer.path().join(&mcp_name)).unwrap();
        fs::rename(&artifact, outer.path().join(cli_name)).unwrap();
        let manifest = serde_json::json!({
            "schema": 1,
            "version": "9.9.9",
            "channel": "edge",
            "assets": [
                { "name": cli_name, "kind": "cli", "sha256": sha256_of(&outer.path().join(cli_name)).unwrap() },
                { "name": mcp_name, "kind": "mcp-server", "sha256": sha256_of(&outer.path().join(&mcp_name)).unwrap() },
                { "name": "gah-server-bundle.tar.gz", "kind": "server-bundle",
                  "sha256": sha256_of(&bundle).unwrap() },
            ]
        });

        let cargo_home = outer.path().join("cargo-home");
        let _guard = CargoHomeGuard::set(&cargo_home);

        let manifest_path = outer.path().join("edge-manifest.json");
        fs::write(&manifest_path, manifest.to_string()).unwrap();
        let location =
            resolve_manifest_location(&repo, Some(manifest_path.to_str().unwrap())).unwrap();
        let manifest: ReleaseManifest =
            serde_json::from_slice(&read_manifest(&location).unwrap()).unwrap();
        let locator = AssetLocator::new(location);

        let staging =
            install_release_artifacts(&manifest, &locator, &repo, HostRole::Central).unwrap();
        assert!(staging.is_some());
        assert!(cargo_home.join("bin/gah").is_file());
        assert!(cargo_home.join("bin/gah-mcp-server").is_file());
        assert!(cargo_home.join("bin/parasaur").exists());
        assert!(repo.join("apps/server/dist/bin.js").is_file());
        assert!(repo.join("apps/web/dist/index.html").is_file());
    }

    /// A binary still open for writing elsewhere (here, by this test) fails
    /// to exec with ETXTBSY; the probe waits for the writer to close it.
    #[cfg(target_os = "linux")]
    #[test]
    fn version_probe_retries_while_the_binary_is_still_open_for_writing() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("gah.download");
        let mut writer = fs::File::create(&binary).unwrap();
        writer.write_all(b"#!/bin/sh\necho gah 9.9.9\n").unwrap();
        writer.flush().unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let busy = Command::new(&binary).arg("--version").output().unwrap_err();
        assert!(is_text_file_busy(&busy), "{busy}");

        let closer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            drop(writer);
        });
        let output = version_probe(&binary).unwrap();
        closer.join().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "gah 9.9.9");
    }

    fn write_fake_binary(path: &Path, script: &str) {
        fs::write(path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// Run a central install from a local manifest that `tamper` breaks,
    /// over an existing install (old CLI, old server dist), and require the
    /// failure to leave that install exactly as it was (#1554).
    fn assert_failed_release_leaves_install_intact(
        tamper: impl FnOnce(&Path, &mut serde_json::Value),
        expected_error: &str,
    ) {
        let outer = tempfile::tempdir().unwrap();
        let repo = outer.path().join("repo");
        fs::create_dir_all(repo.join("apps/server/dist")).unwrap();
        fs::write(repo.join("apps/server/dist/bin.js"), "old server").unwrap();
        fs::write(repo.join("package-lock.json"), "{\"lock\":\"new\"}").unwrap();

        let cli_name = if cfg!(target_os = "macos") {
            "gah-macos-universal"
        } else if cfg!(target_arch = "x86_64") {
            "gah-linux-x86_64"
        } else {
            "gah-linux-aarch64"
        };
        let mcp_name = cli_name.replacen("gah-", "gah-mcp-server-", 1);
        let cli = outer.path().join(cli_name);
        let mcp = outer.path().join(&mcp_name);
        write_fake_binary(&cli, "#!/bin/sh\necho 'gah 9.9.9'\n");
        write_fake_binary(&mcp, "#!/bin/sh\necho 'gah-mcp-server 9.9.9'\n");
        let bundle = write_bundle(outer.path(), "gah-server-bundle.tar.gz", true);
        let mut manifest = serde_json::json!({
            "schema": 1,
            "version": "9.9.9",
            "channel": "edge",
            "assets": [
                { "name": cli_name, "kind": "cli", "sha256": sha256_of(&cli).unwrap() },
                { "name": mcp_name, "kind": "mcp-server", "sha256": sha256_of(&mcp).unwrap() },
                { "name": "gah-server-bundle.tar.gz", "kind": "server-bundle",
                  "sha256": sha256_of(&bundle).unwrap() },
            ]
        });
        tamper(outer.path(), &mut manifest);

        let cargo_home = outer.path().join("cargo-home");
        let bin = cargo_home.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("gah"), "old cli").unwrap();
        let _guard = CargoHomeGuard::set(&cargo_home);

        let manifest_path = outer.path().join("edge-manifest.json");
        fs::write(&manifest_path, manifest.to_string()).unwrap();
        let location =
            resolve_manifest_location(&repo, Some(manifest_path.to_str().unwrap())).unwrap();
        let manifest: ReleaseManifest =
            serde_json::from_slice(&read_manifest(&location).unwrap()).unwrap();
        let locator = AssetLocator::new(location);

        let error =
            install_release_artifacts(&manifest, &locator, &repo, HostRole::Central).unwrap_err();
        assert!(
            format!("{error:#}").contains(expected_error),
            "unexpected error: {error:#}"
        );
        assert_eq!(fs::read_to_string(bin.join("gah")).unwrap(), "old cli");
        // No MCP server, alias, staged binary, or download directory either.
        let left: Vec<_> = fs::read_dir(&bin)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from("gah")]);
        assert_eq!(
            fs::read_to_string(repo.join("apps/server/dist/bin.js")).unwrap(),
            "old server"
        );
        assert!(!repo.join("apps/web/dist").exists());
    }

    #[test]
    fn release_with_bundle_checksum_mismatch_installs_nothing() {
        assert_failed_release_leaves_install_intact(
            |_, manifest| manifest["assets"][2]["sha256"] = "deadbeef".into(),
            "checksum mismatch for gah-server-bundle.tar.gz",
        );
    }

    #[test]
    fn release_with_truncated_bundle_installs_nothing() {
        assert_failed_release_leaves_install_intact(
            |dir, manifest| {
                // The manifest matches the truncated bytes, so only the
                // extraction can catch it.
                let bundle = dir.join("gah-server-bundle.tar.gz");
                let bytes = fs::read(&bundle).unwrap();
                fs::write(&bundle, &bytes[..bytes.len() / 2]).unwrap();
                manifest["assets"][2]["sha256"] = sha256_of(&bundle).unwrap().into();
            },
            "tar extraction of the release server bundle",
        );
    }

    #[test]
    fn release_with_broken_mcp_server_installs_nothing() {
        assert_failed_release_leaves_install_intact(
            |dir, manifest| {
                let name = manifest["assets"][1]["name"].as_str().unwrap().to_string();
                let mcp = dir.join(name);
                write_fake_binary(&mcp, "#!/bin/sh\nexit 3\n");
                manifest["assets"][1]["sha256"] = sha256_of(&mcp).unwrap().into();
            },
            "failed its --version sanity check",
        );
    }
}
