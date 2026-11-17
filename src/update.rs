//! Deterministic CLI and control-plane server update workflow.
//!
//! `cargo build --release` only updates `target/release/gah`. The command a
//! host actually invokes normally lives at `$CARGO_HOME/bin/gah`, so a normal
//! build can silently leave the control plane on old behavior.

use crate::setup::requirements::{linger_path, SYSTEMD_RUNNING};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::fs::{copy, create_dir_all, read_dir, File, OpenOptions};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

mod installation;
mod launch_agent;
mod npm;
mod ownership;
mod reexec;
mod release;
mod units;
pub use installation::installation_plan;
#[cfg(test)]
use installation::quota_refresh_selected;
use installation::NPM_CI_ARGS;
use installation::{agents_to_refresh, install_selected_agent_assets};
use launch_agent::install_macos_launch_agent;

pub use crate::node_role::NodeRole as HostRole;

pub struct UpdateArgs {
    pub repo: Option<PathBuf>,
    pub pull: bool,
    pub agents: Vec<String>,
    pub yes: bool,
    pub role: HostRole,
    pub restart_server: bool,
    pub server_service: String,
    /// Issue #1416: install published release artifacts instead of
    /// rebuilding from source. The checkout stays the deployment root.
    pub from_release: bool,
    /// Explicit manifest (edge-manifest.json) URL or local path; defaults
    /// to the edge feed derived from the checkout's origin remote.
    pub release_manifest: Option<String>,
    /// Issue #1626: when the rendered `gah-server.service` changes `User=`,
    /// repair ownership of the server-managed paths to the new account
    /// (the default); `--no-repair-ownership` refuses the change instead.
    pub repair_ownership: bool,
}

pub fn run(args: UpdateArgs) -> Result<()> {
    units::UnitValues::ensure_installing_account()?;
    if args.role == HostRole::Worker && args.restart_server {
        bail!("--restart-server requires --role central or standalone; a worker never runs gah-server.service");
    }

    let repo = resolve_repo(args.repo.as_deref())?;
    ownership::preflight(
        &repo,
        args.role,
        &args.server_service,
        args.repair_ownership,
    )?;
    let update_lock = acquire_update_lock(&repo)?;
    let maintenance_lock = crate::agents::maintenance_lock()?;
    // Set when this process is the newly installed CLI finishing an update
    // the previous binary started; the plan was already accepted.
    let resumed = env::var_os(reexec::ENV).is_some();
    let config_home = user_config_home()?;
    let agents = agents_to_refresh(&config_home, &args.agents);

    if args.from_release {
        if args.restart_server {
            ensure_no_running_loop_before_server_restart()?;
        }
        // Agent integrations and their quota units come from the checkout
        // on both update paths.
        install_selected_agent_assets(&repo, &config_home, &agents)?;
        return release::run_release(release::ReleaseArgs {
            repo,
            role: args.role,
            restart_server: args.restart_server,
            server_service: args.server_service,
            manifest: args.release_manifest,
            repair_ownership: args.repair_ownership,
        });
    }

    if args.pull && !resumed {
        ensure_default_branch_checkout(&repo)?;
        ensure_clean(&repo)?;
    }
    let plan = installation_plan(args.role, &agents)?;
    if !resumed {
        installation::confirm(&plan, &args, &repo)?;
    }
    if args.restart_server {
        ensure_no_running_loop_before_server_restart()?;
    }

    println!(
        "Updating Parasaur CLI/control plane from {}",
        repo.display()
    );
    if args.pull && !resumed {
        run_command(&repo, "git", &["fetch", "origin", "--prune"])?;
        run_command(&repo, "git", &["pull", "--ff-only"])?;
    }

    // This is the authoritative CLI deployment step. It replaces the Cargo
    // executable selected by PATH, unlike a target/release-only build.
    if !resumed {
        ensure_lockfile_current(&repo)?;
        run_command(
            &repo,
            "cargo",
            &[
                "install",
                "--path",
                ".",
                "--bin",
                "gah",
                "--bin",
                "gah-mcp-server",
                "--force",
                "--locked",
            ],
        )?;
    }

    let binary = installed_binary_path()?;
    if !binary.is_file() {
        bail!(
            "cargo install completed but expected executable is missing: {}",
            binary.display()
        );
    }
    run_command(&repo, binary.to_string_lossy().as_ref(), &["--help"])?;
    println!("Installed CLI: {}", binary.display());
    let alias = installation::install_parasaur_alias(&binary)?;
    println!("Installed CLI alias: {}", alias.display());
    // Everything after this point (unit rendering, installers) is code that
    // the pull may have changed. Finish with the binary just installed, not
    // this older one: an old binary once installed an unrendered
    // gah-server.service and the restart failed.
    // The checkout may be newer than this binary whether or not --pull ran
    // (`git pull && gah update` is the common case), so always hand over.
    if !resumed {
        drop(update_lock);
        drop(maintenance_lock);
        return reexec::continue_with(&binary, &args, &repo);
    }
    // Both roles install user units below; keep them alive past logout. Done
    // early so a later failed step cannot skip it.
    enable_user_lingering(&repo, args.role);

    install_selected_agent_assets(&repo, &config_home, &agents)?;

    if cfg!(target_os = "macos") && args.role == HostRole::Worker {
        run_command(&repo, "npm", NPM_CI_ARGS)?;
    }

    if matches!(args.role, HostRole::Central | HostRole::Standalone) {
        // The control-plane server is part of the MVP; web/desktop/mobile
        // clients intentionally have independent release workflows. A
        // worker node dispatches jobs only and never serves this.
        run_command(&repo, "npm", NPM_CI_ARGS)?;
        run_command(&repo, "npm", &["run", "build:server"])?;
        if !repo.join("apps/server/dist/bin.js").is_file() {
            bail!("server build did not produce apps/server/dist/bin.js");
        }
        // The MCP server is a Rust binary of this crate: `cargo install`
        // above already placed it next to `gah`.
        let mcp_server = binary.with_file_name("gah-mcp-server");
        if !mcp_server.is_file() {
            bail!(
                "cargo install completed but expected executable is missing: {}",
                mcp_server.display()
            );
        }
        println!("Installed MCP server: {}", mcp_server.display());
        println!(
            "Built server:  {}",
            repo.join("apps/server/dist/bin.js").display()
        );

        // Issue #894: keep the system-level control-plane unit in lockstep
        // with the installed server, exactly like the user units below. The
        // gah-server.service is a system unit (not user), so it needs sudo.
        // Operator customization belongs in `systemctl edit gah-server.service`
        // drop-ins; replacing the base template on every update is what
        // prevents the stale-unit drift this fix exists for.
        match install_server_unit_template(&repo, &args.server_service, args.repair_ownership)? {
            Some(target) => println!("Installed server unit: {}", target.display()),
            None => {
                println!("systemd not available on this host: skipping gah-server.service install.")
            }
        }

        match install_prune_unit_template(&repo)? {
            Some(units) => {
                for unit in &units {
                    println!("Installed prune unit: {}", unit.display());
                }
                println!("Daily prune and chat maintenance timer is installed and enabled.");
            }
            None => {
                println!("systemd not available on this host: skipping gah-prune.timer install.")
            }
        }

        // The server serves the dashboard from the checkout's build (#1327).
        // A host with its own web server also gets a copy in that server's
        // root; see `resolve_web_deploy_root`.
        if cfg!(target_os = "macos") {
            run_command(&repo, "npm", WEB_BUILD_ARGS)?;
            println!(
                "Built web UI for the macOS control-plane service: {}",
                repo.join("apps/web/dist").display()
            );
        } else {
            match deploy_web_ui(&repo)? {
                Some(root) => println!("Deployed web UI to {}", root.display()),
                None => println!("Built web UI; gah-server serves it from apps/web/dist."),
            }
        }
    } else if cfg!(target_os = "macos") {
        // The macOS worker exposes the same execution API as the WSL worker.
        // It does not build the central-only MCP server or web dashboard.
        run_command(&repo, "npm", &["run", "build:server"])?;
        if !repo.join("apps/server/dist/bin.js").is_file() {
            bail!("worker build did not produce apps/server/dist/bin.js");
        }
        println!(
            "Built worker server: {}",
            repo.join("apps/server/dist/bin.js").display()
        );
    } else {
        println!("Role is 'worker': skipping control-plane server build on this host.");
    }

    finish_update(&repo, args.role, &args.server_service, args.restart_server)
}

/// The shared tail of both update paths (source and release, issue #1416):
/// macOS service definitions, the systemd user units that must stay in
/// lockstep with the installed CLI, and the optional control-plane restart.
/// Which artifacts got installed differs per path; what must happen to the
/// host afterward does not.
fn finish_update(
    repo: &Path,
    role: HostRole,
    server_service: &str,
    restart_server: bool,
) -> Result<()> {
    if cfg!(target_os = "macos") {
        let script = repo.join("scripts/install-macos-desktop.sh");
        run_command(
            repo,
            "bash",
            &[
                script.to_string_lossy().as_ref(),
                repo.to_string_lossy().as_ref(),
            ],
        )?;
    }

    if let Some(agent) = install_macos_launch_agent(repo, role)? {
        println!("Installed macOS LaunchAgent: {}", agent.display());
    }

    match install_loop_unit_template(repo)? {
        Some(loop_unit) => println!("Installed loop unit: {}", loop_unit.display()),
        None if cfg!(target_os = "macos") => {
            println!("macOS worker lifecycle is owned by its LaunchAgent.")
        }
        None => println!(
            "systemd not available on this host: skipping gah-loop@.service install. \
             Run `gah loop --profile <p>` directly, or wire it into whatever this host \
             uses for supervised long-running processes."
        ),
    }
    match install_watchdog_unit_template(repo)? {
        Some(watchdog_units) => {
            for unit in &watchdog_units {
                println!("Installed watchdog unit: {}", unit.display());
            }
            println!(
                "Watchdog timer is installed but not enabled; the packaged check writes \
                 alerts to the journal. Opt in with `systemctl --user enable --now \
                 gah-watchdog.timer`, then forward alerts by configuring ExecStart in a \
                 `systemctl --user edit gah-watchdog.service` drop-in."
            );
        }
        None => println!("systemd not available on this host: skipping watchdog unit install."),
    }
    crate::factory::keep_services_off_after_update();

    if restart_server && cfg!(target_os = "macos") {
        let script = repo.join("scripts/macos-launchd.sh");
        run_command(
            repo,
            "bash",
            &[script.to_string_lossy().as_ref(), "start", "central"],
        )?;
        println!("Restarted macOS control-plane LaunchAgent.");
    } else if restart_server {
        run_command(repo, "sudo", &["systemctl", "daemon-reload"])?;
        run_command(repo, "sudo", &["systemctl", "restart", server_service])?;
        run_command(repo, "systemctl", &["is-active", "--quiet", server_service])?;
        println!("Restarted service: {}", server_service);
    } else if matches!(role, HostRole::Central | HostRole::Standalone) && !cfg!(target_os = "macos")
    {
        println!(
            "Server not restarted; pass --restart-server when this host serves the control plane."
        );
    }
    Ok(())
}

/// Best-effort probe, not a hard dependency check: a missing `systemctl`
/// (e.g. macOS, containers without systemd) means unit installation is
/// skipped rather than failing the whole update.
fn systemd_available() -> bool {
    Command::new("systemctl")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn resolve_repo(repo: Option<&Path>) -> Result<PathBuf> {
    let path = repo
        .map(Path::to_path_buf)
        .unwrap_or(env::current_dir().context("reading current directory")?);
    let output = Command::new("git")
        .arg("-C")
        .arg(&path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("resolving repository root")?;
    if !output.status.success() {
        bail!(
            "{} is not a Git checkout: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(PathBuf::from(
        String::from_utf8(output.stdout)
            .context("repository root was not UTF-8")?
            .trim(),
    ))
}

fn ensure_default_branch_checkout(repo: &Path) -> Result<()> {
    let branch = captured(repo, "git", &["branch", "--show-current"])?;
    let default_ref = captured(
        repo,
        "git",
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .map_err(|_| {
        anyhow::anyhow!(
            "cannot determine origin's default branch; run `git remote set-head origin -a` in {} before updating",
            repo.display()
        )
    })?;
    let default_branch = default_ref.strip_prefix("origin/").unwrap_or(&default_ref);
    if branch != default_branch {
        bail!(
            "refusing to update non-default branch '{branch}' (origin default is '{default_branch}'); switch to the default branch first"
        );
    }
    Ok(())
}

/// Serialize all mutable update steps (`git pull`, `cargo install`, and `npm
/// ci`) for one checkout. Unlike a profile lock this is deliberately repo-wide:
/// two operators updating the same source tree would otherwise race on Git and
/// dependency state before any GAH profile exists.
fn acquire_update_lock(repo: &Path) -> Result<File> {
    let git_dir = captured(repo, "git", &["rev-parse", "--absolute-git-dir"])?;
    let lock_path = PathBuf::from(git_dir).join("gah-update.lock");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("opening update lock {}", lock_path.display()))?;
    lock.try_lock_exclusive().map_err(|_| {
        anyhow::anyhow!(
            "another `gah update` is already running for {}; wait for it to finish",
            repo.display()
        )
    })?;
    Ok(lock)
}

/// The loop is an independent systemd user unit. Refuse a server restart
/// while one is active so an update cannot change the control plane beneath
/// ongoing work; the operator must intentionally stop it first.
fn ensure_no_running_loop_before_server_restart() -> Result<()> {
    let output = Command::new("pgrep")
        .args(["-af", "gah loop --profile"])
        .output()
        .context("checking for active gah loops before server restart")?;
    if output.status.code() == Some(1) {
        return Ok(());
    }
    if !output.status.success() {
        bail!(
            "could not determine whether a gah loop is active before restarting the server: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let processes = String::from_utf8_lossy(&output.stdout).trim().to_string();
    bail!(
        "refusing to restart gah-server.service while a gah loop is active:\n{processes}\nStop the loop cleanly from the dashboard or `gah loop` owner, then rerun `gah update --restart-server`."
    );
}

/// Resolve the platform-neutral root for per-user configuration assets.
fn user_config_home() -> Result<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .context("HOME or XDG_CONFIG_HOME is required to install user configuration")
}

fn copy_opencode_agent_configs(repo: &Path, config_home: &Path) -> Result<[PathBuf; 2]> {
    copy_opencode_agent_configs_from(&repo.join("packaging/opencode/agents"), config_home)
}

/// Issue #1416: the release path passes the extracted bundle's copy of
/// `packaging/opencode/agents` here instead of the (possibly stale)
/// checkout's, because release mode never runs `git pull`.
fn copy_opencode_agent_configs_from(source_dir: &Path, config_home: &Path) -> Result<[PathBuf; 2]> {
    let target_dir = config_home.join("opencode/agents");
    create_dir_all(&target_dir)
        .with_context(|| format!("creating OpenCode agent directory {}", target_dir.display()))?;

    let mut installed = Vec::with_capacity(2);
    for name in ["gah-reviewer.md", "gah-implementer.md"] {
        let source = source_dir.join(name);
        if !source.is_file() {
            bail!("OpenCode agent config is missing: {}", source.display());
        }
        let target = target_dir.join(name);
        copy(&source, &target).with_context(|| {
            format!(
                "installing OpenCode agent from {} to {}",
                source.display(),
                target.display()
            )
        })?;
        installed.push(target);
    }

    Ok(installed
        .try_into()
        .expect("two OpenCode agents are installed"))
}

/// Keep the dashboard's lifecycle unit in lockstep with the installed CLI and
/// control plane. Local operator customization belongs in `systemctl --user
/// edit gah-loop@<profile>` drop-ins, so replacing this base template on every
/// deterministic update is safe and prevents source/runtime ownership drift.
fn copy_systemd_unit(repo: &Path, config_home: &Path, unit_file_name: &str) -> Result<PathBuf> {
    let source = repo.join("packaging/systemd").join(unit_file_name);
    if !source.is_file() {
        bail!("systemd unit template is missing: {}", source.display());
    }
    let target = config_home.join("systemd/user").join(unit_file_name);
    let parent = target.parent().expect("systemd unit target has a parent");
    create_dir_all(parent)
        .with_context(|| format!("creating systemd user-unit directory {}", parent.display()))?;
    let template = std::fs::read_to_string(&source)
        .with_context(|| format!("reading systemd unit template {}", source.display()))?;
    let rendered = units::render(&template, &units::UnitValues::resolve(repo)?)
        .with_context(|| format!("rendering {unit_file_name}"))?;
    std::fs::write(&target, rendered).with_context(|| {
        format!(
            "installing systemd unit from {} to {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(target)
}

/// Issue #894: keep the system-level control-plane unit in lockstep with the
/// installed server, same reasoning as `install_loop_unit_template`. Unlike
/// the user units, gah-server.service is a system unit owned by root, so
/// install needs `sudo`; operator customization belongs in `systemctl edit
/// gah-server.service` drop-ins, so replacing the base template on every
/// deterministic update is safe. `--server-service` names the installed unit
/// (default `gah-server.service`), so the copy target matches what the
/// restart step below restarts.
fn install_server_unit_template(
    repo: &Path,
    server_service: &str,
    repair_ownership: bool,
) -> Result<Option<PathBuf>> {
    install_server_unit_template_into(
        repo,
        server_service,
        Path::new("/etc/systemd/system"),
        repair_ownership,
    )
}

/// The install above, parameterized by the systemd unit directory so tests
/// can render over an existing installed unit in a temporary directory
/// (issue #1626: the ownership repair decision reads that unit first).
fn install_server_unit_template_into(
    repo: &Path,
    server_service: &str,
    units_dir: &Path,
    repair_ownership: bool,
) -> Result<Option<PathBuf>> {
    ownership::install_server_unit_into(repo, server_service, units_dir, repair_ownership)
}

/// Where to copy the built dashboard for a separate web server (issue #896),
/// from `PARASAUR_WEB_DEPLOY_ROOT`. The copy needs `sudo`.
///
/// - unset -> `/var/www/gah` when that directory already exists (a host set
///   up before #1327, serving it with Caddy or similar); otherwise no copy,
///   and the GAH server serves the checkout's build without root
/// - set to a non-empty path -> that path
/// - set to empty -> no copy
fn resolve_web_deploy_root(configured: Option<OsString>) -> Result<Option<PathBuf>> {
    let legacy = Path::new("/var/www/gah");
    let Some(root) = configured
        .map(PathBuf::from)
        .or_else(|| legacy.is_dir().then(|| legacy.to_path_buf()))
        .filter(|root| !root.as_os_str().is_empty())
    else {
        return Ok(None);
    };
    if !root.is_absolute()
        || root == Path::new("/")
        || root.components().any(|part| part == Component::ParentDir)
    {
        bail!("PARASAUR_WEB_DEPLOY_ROOT must be an absolute path other than /");
    }
    Ok(Some(root))
}

/// Runs the `apps/web` workspace build directly rather than the root
/// `build:web` script. `build:web` carries an npm `prebuild:web` lifecycle
/// hook that reruns the whole server integration suite -- fine for CI, but
/// on the production host it can hang on a flaky integration test (issue
/// #1010) and wedge the updater before it ever restarts the server. CI still
/// calls `npm run build:web` and keeps exercising that hook; only this
/// deploy path skips it.
const WEB_BUILD_ARGS: &[&str] = &["run", "--workspace=apps/web", "build"];

/// Deploys `apps/web/dist` into the configured web root. Ordering makes the
/// swap atomic for a browser (issue #896 review): Vite content-hashes every
/// asset filename, and `index.html` is the only file a browser loads first,
/// so
///
/// 1. the hashed `assets/` files are copied before anything else,
/// 2. `index.html` is replaced last -- a fetch mid-deploy either gets the
///    old page (old index + old assets, both still present) or the new page
///    (new index + new assets, already on disk), never a new index
///    referencing chunks that don't exist yet,
/// 3. stale hashed assets the new build no longer references are pruned, so
///    the root doesn't accumulate every previous build's output forever.
///
/// Only the build output (`index.html` and `assets/`) is touched;
/// operator-provided files in the root (favicon overrides, robots.txt, a
/// web-server config file) survive the update.
fn deploy_web_ui(repo: &Path) -> Result<Option<PathBuf>> {
    // Always build: the server serves this directory when nothing is copied.
    run_command(repo, "npm", WEB_BUILD_ARGS)?;
    let dist = repo.join("apps/web/dist");
    if !dist.join("index.html").is_file() {
        bail!("web build did not produce apps/web/dist/index.html");
    }
    let Some(root_path) = resolve_web_deploy_root(env::var_os("PARASAUR_WEB_DEPLOY_ROOT"))? else {
        return Ok(None);
    };
    deploy_web_dist(repo, &dist, root_path)
}

/// Issue #1416: the release path calls this directly with the freshly
/// extracted `apps/web/dist` instead of building it -- same deploy ordering
/// and pruning, no npm.
fn deploy_web_dist(repo: &Path, dist: &Path, root_path: PathBuf) -> Result<Option<PathBuf>> {
    if !dist.join("index.html").is_file() {
        bail!("web build did not produce apps/web/dist/index.html");
    }
    let root = root_path
        .to_str()
        .context("PARASAUR_WEB_DEPLOY_ROOT is not UTF-8")?;
    run_command(
        repo,
        "sudo",
        &[
            "install", "-d", "-o", "root", "-g", "root", "-m", "0755", root,
        ],
    )?;

    // 1. Hashed assets land first, so a new index.html never references
    //    chunks that aren't on disk yet.
    let dist_assets = dist.join("assets");
    if dist_assets.is_dir() {
        run_command(
            repo,
            "sudo",
            &[
                "install",
                "-d",
                "-o",
                "root",
                "-g",
                "root",
                "-m",
                "0755",
                &format!("{root}/assets"),
            ],
        )?;
        let dist_assets = dist_assets
            .to_str()
            .context("web dist assets path is not UTF-8")?;
        run_command(
            repo,
            "sudo",
            &["cp", "-r", "--", dist_assets, &format!("{root}/")],
        )?;
        // Build/archive umasks must not make public assets unreadable to the
        // unprivileged server after this root-owned deployment.
        run_command(
            repo,
            "sudo",
            &["chmod", "-R", "a+rX", "--", &format!("{root}/assets")],
        )?;
    }

    // 2. index.html last: the browser-facing swap is a single-file replace.
    let dist_index = dist.join("index.html");
    let dist_index = dist_index
        .to_str()
        .context("web dist index path is not UTF-8")?;
    let staged_index = format!("{root}/.gah-index.html.new");
    run_command(
        repo,
        "sudo",
        &["install", "-m", "0644", "--", dist_index, &staged_index],
    )?;
    run_command(
        repo,
        "sudo",
        &[
            "mv",
            "-f",
            "--",
            &staged_index,
            &format!("{root}/index.html"),
        ],
    )?;

    // 3. Prune stale hashed assets. Best-effort: a prune hiccup must not
    //    fail the whole update after the new page is already live.
    prune_stale_web_assets(repo, &root_path, dist);

    Ok(Some(root_path))
}

/// Removes files under `<root>/assets/` that the new build no longer
/// references. Vite content-hashes asset filenames, so any file present in
/// the deployed assets dir but missing from the freshly built one is stale.
/// Non-fatal: a listing/removal failure is logged but does not roll back the
/// deploy, which has already replaced `index.html`.
fn prune_stale_web_assets(repo: &Path, root: &Path, dist: &Path) {
    let dist_assets = dist.join("assets");
    if !dist_assets.is_dir() {
        return;
    }
    let keep: HashSet<String> = match read_dir(&dist_assets) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect(),
        Err(error) => {
            eprintln!("[gah update] cannot read new web assets to prune against: {error}");
            return;
        }
    };
    let root_assets = root.join("assets");
    let Some(root_assets_str) = root_assets.to_str() else {
        eprintln!("[gah update] web deploy root is not UTF-8; skipping stale-asset prune");
        return;
    };
    let listing = match captured(
        repo,
        "sudo",
        &["find", root_assets_str, "-maxdepth", "1", "-type", "f"],
    ) {
        Ok(listing) => listing,
        // assets/ doesn't exist yet or isn't listable -- nothing to prune.
        Err(error) => {
            eprintln!("[gah update] cannot list deployed web assets to prune: {error}");
            return;
        }
    };
    let stale = stale_asset_names(&listing, &keep);
    if stale.is_empty() {
        return;
    }
    let mut args = vec!["rm", "-f", "--"];
    let mut names = Vec::new();
    for name in stale {
        names.push(format!("{root_assets_str}/{name}"));
    }
    args.extend(names.iter().map(String::as_str));
    if let Err(error) = run_command(repo, "sudo", &args) {
        eprintln!("[gah update] failed to prune stale web assets: {error}");
    }
}

/// Pure filter: which files in a `find`-style listing (one path per line)
/// are NOT part of the freshly built assets set. Content-hashed names mean
/// anything missing from the new build is stale. Factored out so the
/// deletion list is unit-testable without a sudo shim.
fn stale_asset_names(listing: &str, keep: &HashSet<String>) -> Vec<String> {
    listing
        .lines()
        .filter_map(|line| Path::new(line).file_name()?.to_str().map(String::from))
        .filter(|name| !keep.contains(name))
        .collect()
}

fn reload_user_systemd(reason: &str) -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .with_context(|| format!("reloading the user systemd manager after {reason}"))?;
    if !status.success() {
        bail!("systemctl --user daemon-reload exited with {status}");
    }
    Ok(())
}

fn install_loop_unit_template(repo: &Path) -> Result<Option<PathBuf>> {
    if !systemd_available() {
        return Ok(None);
    }
    let config_home = user_config_home()?;
    let target = copy_systemd_unit(repo, &config_home, "gah-loop@.service")?;
    reload_user_systemd("installing gah-loop@.service")?;
    Ok(Some(target))
}

/// Issue #726: keep the alert-only watchdog unit in lockstep with the
/// installed CLI, same reasoning as `install_loop_unit_template`. This also
/// safely replaces an old host-local-script-based `gah-watchdog.service`
/// left over from before this unit was tracked in-repo (AC7) -- the copy
/// destination is identical, so a stale version is simply overwritten.
/// Deliberately does not `enable` or `start` the timer: that stays an
/// explicit operator opt-in.
fn install_watchdog_unit_template(repo: &Path) -> Result<Option<[PathBuf; 2]>> {
    if !systemd_available() {
        return Ok(None);
    }
    let config_home = user_config_home()?;
    let service = copy_systemd_unit(repo, &config_home, "gah-watchdog.service")?;
    let timer = copy_systemd_unit(repo, &config_home, "gah-watchdog.timer")?;
    reload_user_systemd("installing gah-watchdog.service/.timer")?;
    Ok(Some([service, timer]))
}

/// Issue #761: keep the account-level quota refresh units in lockstep with
/// the installed CLI, same reasoning as the other unit templates. Unlike the
/// watchdog (which needs an alert command configured before it can do
/// anything), quota refresh works with zero extra config -- it writes a
/// data-free marker for backends that aren't configured and moves on -- so
/// `gah update` both installs AND enables the timer: this is what makes
/// `quota_observations.jsonl` populate automatically on deployed nodes
/// instead of going stale for days (the bug #761 exists to fix). An operator
/// who genuinely doesn't want periodic quota refresh opts out with
/// `systemctl --user disable --now gah-quota-refresh.timer`.
fn install_quota_refresh_unit_template(repo: &Path) -> Result<Option<[PathBuf; 2]>> {
    if !systemd_available() {
        return Ok(None);
    }
    let config_home = user_config_home()?;
    let service = copy_systemd_unit(repo, &config_home, "gah-quota-refresh.service")?;
    let timer = copy_systemd_unit(repo, &config_home, "gah-quota-refresh.timer")?;
    reload_user_systemd("installing gah-quota-refresh.service/.timer")?;
    let status = Command::new("systemctl")
        .args(["--user", "enable", "--now", "gah-quota-refresh.timer"])
        .status()
        .with_context(|| "enabling gah-quota-refresh.timer")?;
    if !status.success() {
        bail!("systemctl --user enable --now gah-quota-refresh.timer exited with {status}");
    }
    Ok(Some([service, timer]))
}

/// Issue #1347: the user units `gah update` installs (loop, quota refresh,
/// prune) run under the user's systemd manager, which only exists while the
/// account has a login session. On a headless node without lingering, every
/// timer stays idle after each reboot until someone logs in. Returns nothing
/// on purpose: a failure is reported, never fatal, and `gah setup --check`
/// keeps the gap visible.
fn enable_user_lingering(repo: &Path, role: HostRole) {
    if !Path::new(SYSTEMD_RUNNING).exists() {
        // launchd hosts and systemd-less machines have no linger concept.
        return;
    }
    let Some(user) = units::installing_user() else {
        eprintln!("[gah update] user lingering skipped: id -un named no account.");
        return;
    };
    if linger_path(&user).exists() {
        return;
    }
    if try_enable_linger(repo, &user, role == HostRole::Central) {
        println!("Enabled user lingering for {user}: user units survive logouts and reboots.");
    } else {
        eprintln!(
            "[gah update] user lingering not enabled; the user units stay idle after \
             reboot until someone logs in. Fix with: sudo loginctl enable-linger {user}"
        );
    }
}

/// Plain loginctl works where polkit allows `set-self-linger` (not over SSH
/// on Debian 13, for one); its expected refusal is kept quiet. Then sudo:
/// central already asks for the sudo password to install its system unit,
/// so it may ask here too; a worker uses `sudo -n` and never waits on one.
fn try_enable_linger(repo: &Path, user: &str, may_prompt: bool) -> bool {
    let polkit = Command::new("loginctl")
        .args(["--no-ask-password", "enable-linger"])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    let sudo: &[&str] = if may_prompt {
        &["loginctl", "enable-linger", user]
    } else {
        &["-n", "loginctl", "enable-linger", user]
    };
    polkit || run_command(repo, "sudo", sudo).is_ok()
}

/// Central-node daily storage maintenance. The existing timer owns both the
/// Rust prune and server chat sweep; workers never install this control-plane
/// entrypoint and no second scheduler is introduced.
fn install_prune_unit_template(repo: &Path) -> Result<Option<[PathBuf; 2]>> {
    if !systemd_available() {
        return Ok(None);
    }
    let config_home = user_config_home()?;
    let service = copy_systemd_unit(repo, &config_home, "gah-prune.service")?;
    let timer = copy_systemd_unit(repo, &config_home, "gah-prune.timer")?;
    reload_user_systemd("installing gah-prune.service/.timer")?;
    let status = Command::new("systemctl")
        .args(["--user", "enable", "--now", "gah-prune.timer"])
        .status()
        .with_context(|| "enabling gah-prune.timer")?;
    if !status.success() {
        bail!("systemctl --user enable --now gah-prune.timer exited with {status}");
    }
    Ok(Some([service, timer]))
}

fn ensure_clean(repo: &Path) -> Result<()> {
    let status = captured(repo, "git", &["status", "--porcelain"])?;
    if !status.is_empty() {
        bail!(
            "refusing to update a dirty checkout; commit, stash, or move these changes first:\n{status}"
        );
    }
    Ok(())
}

pub use installation::heal_parasaur_alias;

fn installed_binary_path() -> Result<PathBuf> {
    let cargo_home = match env::var_os("CARGO_HOME") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(
            env::var_os("HOME").context("HOME is required to locate Cargo-installed gah")?,
        )
        .join(".cargo"),
    };
    Ok(cargo_home.join("bin").join("gah"))
}

fn captured(repo: &Path, program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo)
        .output()
        .with_context(|| format!("starting {program} {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "{program} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_string())
        .context("command output was not UTF-8")
}

/// `cargo install --locked` silently re-resolves a Cargo.lock that no longer
/// matches Cargo.toml; `cargo metadata --locked` refuses. Check first so an
/// update never builds a dependency set CI did not test.
fn ensure_lockfile_current(repo: &Path) -> Result<()> {
    let output = Command::new("cargo")
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(repo)
        .output()
        .context("starting cargo metadata --locked")?;
    if !output.status.success() {
        bail!(
            "Cargo.lock in {} does not match Cargo.toml; refusing to install untested dependency versions. Commit an updated Cargo.lock, then run the update again.\n{}",
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn run_command(repo: &Path, program: &str, args: &[&str]) -> Result<()> {
    println!("> {} {}", program, args.join(" "));
    let mut command = if program == "npm" {
        npm::command()?
    } else {
        Command::new(program)
    };
    let status = command
        .args(args)
        .current_dir(repo)
        .status()
        .with_context(|| format!("starting {program} {}", args.join(" ")))?;
    if !status.success() {
        bail!("{program} {} exited with {status}", args.join(" "));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
