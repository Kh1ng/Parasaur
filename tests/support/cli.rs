#![allow(dead_code)]

#[cfg(unix)]
use std::ffi::CString;
use std::fs;
use std::ops::{Deref, DerefMut};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::TempDir;

#[cfg(unix)]
const TEST_INTEGRATION_MIN_DISPATCH_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// Keep integration-test repositories and child-process temporary files on
/// the same filesystem as Cargo's build output. Some development hosts mount
/// `/tmp` as a small tmpfs; production dispatch capacity checks must remain
/// strict without making the test suite depend on that unrelated filesystem's
/// current free-space level.
pub fn test_temp_root() -> PathBuf {
    let fallback_root = std::env::temp_dir();
    let default_root = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent()?.parent().map(Path::to_path_buf))
        })
        .unwrap_or_else(|| fallback_root.clone());
    let candidates = [
        default_root.clone(),
        PathBuf::from("/dev/shm"),
        fallback_root.clone(),
    ];
    let root = candidates
        .into_iter()
        .find(|candidate| has_minimum_dispatch_space(candidate))
        .unwrap_or(default_root)
        .join("gah-integration-test-tmp");
    fs::create_dir_all(&root).unwrap();
    root
}

#[cfg(unix)]
fn has_minimum_dispatch_space(path: &Path) -> bool {
    match free_space_bytes(path) {
        Some(available) => available >= TEST_INTEGRATION_MIN_DISPATCH_BYTES,
        None => false,
    }
}

#[cfg(unix)]
fn free_space_bytes(path: &Path) -> Option<u64> {
    let filesystem_path = path.ancestors().find(|ancestor| ancestor.exists())?;
    let path_c = CString::new(filesystem_path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path_c.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(not(unix))]
fn has_minimum_dispatch_space(_path: &Path) -> bool {
    true
}

pub fn test_tempdir() -> TempDir {
    tempfile::Builder::new()
        .prefix("gah-test-")
        .tempdir_in(test_temp_root())
        .unwrap()
}

/// Retains a command's isolated filesystem environment until the command is
/// finished, then removes it with the fixture. This matters for asynchronously
/// spawned children: dropping the temporary directory immediately after
/// `spawn()` would race the child while leaving anything it recreated behind.
pub struct IsolatedCommand<C> {
    command: C,
    state: TempDir,
}

impl<C> IsolatedCommand<C> {
    pub fn state_path(&self) -> &Path {
        self.state.path()
    }
}

impl<C> Deref for IsolatedCommand<C> {
    type Target = C;

    fn deref(&self) -> &Self::Target {
        &self.command
    }
}

impl<C> DerefMut for IsolatedCommand<C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.command
    }
}

pub fn isolate_command<C>(
    mut command: C,
    set_env: impl FnOnce(&mut C, &Path),
) -> IsolatedCommand<C> {
    let state = test_tempdir();
    set_env(&mut command, state.path());
    IsolatedCommand { command, state }
}

pub trait TestCommandEnvironment {
    fn env_path(&mut self, key: &str, value: &Path);
    fn env_unset(&mut self, key: &str);
}

impl TestCommandEnvironment for assert_cmd::Command {
    fn env_path(&mut self, key: &str, value: &Path) {
        self.env(key, value);
    }

    fn env_unset(&mut self, key: &str) {
        self.env_remove(key);
    }
}

impl TestCommandEnvironment for std::process::Command {
    fn env_path(&mut self, key: &str, value: &Path) {
        self.env(key, value);
    }

    fn env_unset(&mut self, key: &str) {
        self.env_remove(key);
    }
}

/// Write the node-pressure fixture every `gah` child gets unless its test
/// sets its own: an idle 64 GiB node, so node admission never depends on
/// the free memory or load of the host running the tests.
pub fn write_idle_node_pressure(path: &Path) {
    fs::write(
        path,
        serde_json::json!({
            "memory_total_bytes": 64_u64 * 1024 * 1024 * 1024,
            "memory_available_bytes": 60_u64 * 1024 * 1024 * 1024,
            "logical_cpus": 8, "load_one": 0.0,
            "memory_full_psi_avg10": 0.0, "cpu_some_psi_avg10": 0.0
        })
        .to_string(),
    )
    .unwrap();
}

/// Variables that make a child's anyhow errors end in a stack backtrace. A
/// developer shell often exports them; the backtrace then pushes the error
/// text that tests assert on out of view, so `gah` children never inherit
/// them.
pub const BACKTRACE_ENV_VARS: [&str; 2] = ["RUST_BACKTRACE", "RUST_LIB_BACKTRACE"];

/// Isolate all process-global GAH state used by CLI integration children,
/// including the node-wide capacity lease registry. CLI integration tests
/// may run under the real systemd loop, which sets XDG_STATE_HOME to the
/// operator's persistent state directory; never let fake profiles and work
/// claims leak into (or inherit from) that state. Node admission reads an
/// idle pressure fixture instead of the live host; a test that sets
/// `GAH_TEST_NODE_PRESSURE_FILE` on the returned command overrides it.
pub fn isolate_gah_command<C: TestCommandEnvironment>(command: C) -> IsolatedCommand<C> {
    isolate_command(command, |command, root| {
        let tmp = root.join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        command.env_path("HOME", &root.join("home"));
        command.env_path("GAH_CANONICAL_CONFIG", &root.join("canonical.toml"));
        command.env_path("XDG_STATE_HOME", &root.join("xdg-state"));
        command.env_path("XDG_RUNTIME_DIR", &root.join("xdg-runtime"));
        command.env_path("GAH_AVAILABILITY_PATH", &root.join("availability.json"));
        command.env_path(
            "GAH_VALIDATION_CHECK_PATH",
            &root.join("validation-check.json"),
        );
        command.env_path("TMPDIR", &tmp);
        let node_pressure = root.join("node-pressure.json");
        write_idle_node_pressure(&node_pressure);
        command.env_path("GAH_TEST_NODE_PRESSURE_FILE", &node_pressure);
        command.env_unset("GAH_TEST_NOW");
        for key in BACKTRACE_ENV_VARS {
            command.env_unset(key);
        }
    })
}

pub fn gah_command() -> IsolatedCommand<assert_cmd::Command> {
    isolate_gah_command(assert_cmd::Command::cargo_bin("gah").unwrap())
}

pub fn gah_process_command() -> IsolatedCommand<std::process::Command> {
    let command = std::process::Command::new(
        std::env::var("CARGO_BIN_EXE_gah").unwrap_or_else(|_| "target/debug/gah".into()),
    );
    isolate_gah_command(command)
}

/// Direct integration-test calls into the production library do not have
/// `cfg(test)`, so scope their canonical lookup to an empty fixture too.
pub fn load_config(path: Option<&str>) -> anyhow::Result<git_agent_harness::config::GahConfig> {
    static CANONICAL_ENV_LOCK: Mutex<()> = Mutex::new(());
    let _lock = CANONICAL_ENV_LOCK.lock().unwrap();
    let isolated = test_tempdir();
    let original = std::env::var_os("GAH_CANONICAL_CONFIG");
    std::env::set_var(
        "GAH_CANONICAL_CONFIG",
        isolated.path().join("canonical.toml"),
    );
    let result = git_agent_harness::config::load(path);
    match original {
        Some(value) => std::env::set_var("GAH_CANONICAL_CONFIG", value),
        None => std::env::remove_var("GAH_CANONICAL_CONFIG"),
    }
    result
}

pub fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

pub fn write_fake_binary(dir: &Path, name: &str, body: &str) {
    write_executable(&dir.join(name), body);
}
