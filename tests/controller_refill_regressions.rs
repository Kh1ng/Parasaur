#[path = "controller_refill_regressions/node_recovery.rs"]
mod node_recovery;
mod support;
#[path = "controller_refill_regressions/watchdog.rs"]
mod watchdog;
#[path = "controller_refill_regressions/worktree_isolation.rs"]
mod worktree_isolation;

use std::fs;
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use support::{test_tempdir, write_fake_binary as make_fake_bin_with_body, ProcessGroupGuard};

// Every test in this file spawns a real `gah` subprocess and does real
// timing-sensitive polling against it (worker admission, node-pressure
// reprobe, shutdown). In isolation each one consistently finishes in a few
// seconds flat -- confirmed empirically, pinned to 2 CPUs (matching CI's
// actual ubuntu-latest runner) and repeated -- but Rust's default test
// harness runs tests within a binary concurrently, and on a CPU-constrained
// runner two of these real-subprocess tests scheduled at the same time
// genuinely starve each other, which is what was actually causing
// parallel_loop_reprobes_node_pressure_with_active_worker_remaining to blow
// past its deadline (issue #824) -- not the deadline being too tight. Widening
// it twice (PR #825: 10s->20s) treated the symptom; this treats the cause.
// Same pattern already used in tests/fake_backend_harness.rs for the same
// class of problem.
static TEST_MUTEX: Mutex<()> = Mutex::new(());

/// Take the serializing lock. A test that fails while holding it poisons the
/// mutex; the lock guards no data, only ordering, so the next test takes it
/// anyway. Without this one timeout fails every test that runs after it with
/// `PoisonError`, which hides which test actually failed.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    TEST_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn spawn_bin(state_root: &std::path::Path) -> ProcessCommand {
    // These tests exercise refill state transitions, not host sizing. The
    // real debug binary accepts this explicit fixture; release binaries never
    // compile the hook and therefore cannot use it to bypass live pressure.
    let node_pressure = state_root.join("node-pressure.json");
    fs::write(
        &node_pressure,
        r#"{
  "memory_total_bytes": 68719476736,
  "memory_available_bytes": 64424509440,
  "logical_cpus": 32,
  "load_one": 0.5,
  "memory_full_psi_avg10": 0.0,
  "cpu_some_psi_avg10": 0.0
}"#,
    )
    .unwrap();
    let mut cmd = ProcessCommand::new(
        std::env::var("CARGO_BIN_EXE_gah").unwrap_or_else(|_| "target/debug/gah".into()),
    );
    fs::create_dir_all(state_root.join("home")).unwrap();
    cmd.env_remove("GAH_TEST_NOW");
    cmd.env("HOME", state_root.join("home"));
    cmd.env("GAH_CANONICAL_CONFIG", state_root.join("canonical.toml"));
    cmd.env("XDG_STATE_HOME", state_root.join("state"));
    cmd.env(
        "GAH_AVAILABILITY_PATH",
        "/nonexistent-availability-path.json",
    );
    cmd.env(
        "GAH_VALIDATION_CHECK_PATH",
        state_root.join("validation.json"),
    );
    // Capacity leases are node-global in production. These integration
    // children use fake workers and run concurrently, so give each test its
    // own registry instead of letting unrelated test processes reserve slots
    // from one another.
    cmd.env("XDG_RUNTIME_DIR", state_root.join("runtime"));
    cmd.env("TMPDIR", support::test_temp_root());
    cmd.env("GAH_TEST_NODE_PRESSURE_FILE", node_pressure);
    cmd.env("GAH_TEST_NODE_CAPACITY_REPROBE_MS", "100");
    // Each dispatch recalls from and captures to the memory gateway, by
    // default http://127.0.0.1:8420. Whatever the host runs there is not part
    // of these tests, and a port that drops connections instead of refusing
    // them costs a 10 s curl timeout per call (#1662). curl fails a file URL
    // at once, and the dispatch carries on as it does with no gateway.
    cmd.env("TDAI_GATEWAY_URL", "file:///nonexistent-memory-gateway");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd
}

fn write_node_pressure_with_load(
    path: &std::path::Path,
    available_gib: u64,
    logical_cpus: u64,
    load_one: f64,
) {
    let replacement = path.with_extension("json.tmp");
    fs::write(
        &replacement,
        format!(
            r#"{{
  "memory_total_bytes": 68719476736,
  "memory_available_bytes": {},
  "logical_cpus": {},
  "load_one": {},
  "memory_full_psi_avg10": 0.0,
  "cpu_some_psi_avg10": 0.0
}}"#,
            available_gib * 1024 * 1024 * 1024,
            logical_cpus,
            load_one
        ),
    )
    .unwrap();
    fs::rename(replacement, path).unwrap();
}

fn read_u32_file(path: &std::path::Path) -> u32 {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Upper bound for an event wait. It exists only so a genuinely stuck loop
/// fails instead of hanging; it is not a performance expectation.
const WORKER_EVENT_TIMEOUT: Duration = Duration::from_secs(120);

/// Wait until `observed()` holds while the controller is still running. On
/// an early controller exit or a stuck loop, release the blocked fake workers
/// and fail with the controller's output so the cause is diagnosable.
fn wait_for_worker_event(
    mut child: ProcessGroupGuard,
    release: &std::path::Path,
    observed: impl Fn() -> bool,
    what: &str,
) -> ProcessGroupGuard {
    let deadline = Instant::now() + WORKER_EVENT_TIMEOUT;
    while !observed() {
        let exited = child.try_wait().unwrap().is_some();
        if exited || Instant::now() >= deadline {
            let reason = if exited {
                "controller exited before refill opportunity".to_string()
            } else {
                format!("timed out after {}s", WORKER_EVENT_TIMEOUT.as_secs())
            };
            let processes = if exited {
                String::new()
            } else {
                watchdog::process_tree(child.id() as i32)
            };
            fs::write(release, "release").unwrap();
            if !exited {
                // A stuck controller would never exit on its own; kill its
                // whole process group so collecting the output cannot hang.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
            }
            let output = child.wait_with_output().unwrap();
            panic!(
                "{what}: {reason}\nprocesses:\n{processes}\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
    child
}

#[cfg(unix)]
#[test]
fn process_group_guard_drop_reaps_the_entire_test_process_group() {
    let _lock = serial();
    use std::os::unix::process::CommandExt;
    let mut command = ProcessCommand::new("/bin/sh");
    command
        .args(["-c", "trap 'exit 0' TERM; sleep 60 & wait"])
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let guard = ProcessGroupGuard::new(command.spawn().unwrap());
    let process_group = guard.id() as i32;

    drop(guard);

    // SIGKILL delivery is synchronous, but a reparented descendant can remain
    // observable as a zombie until the CI runner's subreaper collects it.
    // Bound that kernel/reaper delay instead of assuming kill(2) makes the
    // process-group lookup disappear in the same instruction.
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(-process_group, 0) } == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    let exists = unsafe { libc::kill(-process_group, 0) } == 0;
    assert!(
        !exists,
        "test process group {process_group} survived guard drop"
    );
}

fn init_git_repo(path: &std::path::Path) {
    fs::create_dir_all(path.join("docs")).unwrap();
    ProcessCommand::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .unwrap();
    ProcessCommand::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(path)
        .output()
        .unwrap();
    ProcessCommand::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(path)
        .output()
        .unwrap();
    fs::write(path.join("README.md"), "hello\n").unwrap();
    fs::write(path.join("docs/MANAGER_MEMORY.md"), "# Memory\n").unwrap();
    ProcessCommand::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .unwrap();
    ProcessCommand::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(path)
        .output()
        .unwrap();
}

fn configure_git_url_instead_of(home: &std::path::Path, from: &str, to: &str) {
    fs::write(
        home.join(".gitconfig"),
        format!("[url \"{}\"]\n\tinsteadOf = {}\n", to, from),
    )
    .unwrap();
}

fn write_real_repo_config_with_extra(
    tmp: &tempfile::TempDir,
    repo: &std::path::Path,
    provider: &str,
    extra_profile: &str,
) -> std::path::PathBuf {
    let cfg = tmp.path().join("gah-config-real.toml");
    let extra = match provider {
        "gitlab" => "provider_api_base = \"https://gitlab.example.com/api/v4\"\nprovider_project_id = \"42\"\n",
        _ => "",
    };
    fs::write(
        &cfg,
        format!(
            r#"
[defaults]
artifact_root = "{root}/artifacts"
worktree_base = "{root}/worktrees"
llm_base_url  = "http://localhost:4000"
llm_model_local = "local/test"
llm_model_cloud = "cloud/test"

[profiles.real]
display_name          = "Real Repo"
repo_id               = "real"
provider              = "{provider}"
repo                  = "owner/real"
local_path            = "{repo}"
artifact_root         = "{root}/artifacts/real"
default_target_branch = "main"
{extra}
{extra_profile}
"#,
            root = tmp.path().display(),
            provider = provider,
            repo = repo.display(),
            extra = extra,
            extra_profile = extra_profile,
        ),
    )
    .unwrap();
    cfg
}

fn setup_fix_dispatch_repo(
    tmp: &tempfile::TempDir,
    extra_profile: &str,
) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let repo = tmp.path().join("repo");
    let home = tmp.path().join("home");
    let github_root = tmp.path().join("github-root");
    let origin = github_root.join("owner/real.git");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&home).unwrap();
    init_git_repo(&repo);
    fs::create_dir_all(origin.parent().unwrap()).unwrap();
    ProcessCommand::new("git")
        .args(["init", "--bare", origin.to_str().unwrap()])
        .output()
        .unwrap();
    configure_git_url_instead_of(
        &home,
        "https://github.com/",
        &format!("file://{}/", github_root.display()),
    );
    ProcessCommand::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/owner/real.git",
        ])
        .current_dir(&repo)
        .output()
        .unwrap();
    ProcessCommand::new("git")
        .args(["push", "-u", "origin", "main"])
        .current_dir(&repo)
        .env("HOME", &home)
        .output()
        .unwrap();

    let cfg = write_real_repo_config_with_extra(
        tmp,
        &repo,
        "github",
        &format!(
            "{}\n[profiles.real.routing]\nimprove_backend = \"codex\"\nreview_backend = \"claude\"\n",
            extra_profile
        ),
    );
    (repo, home, cfg)
}

#[test]
// #1539: spawn_bin points availability at /nonexistent-availability-path.json.
// A loop tick must still dispatch and refill when maintenance cannot write there.
fn parallel_loop_refills_with_missing_availability_after_a_fast_completion() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");

    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for (ticket_id, title) in [
        ("401", "Slow slot"),
        ("402", "Fast slot"),
        ("403", "Refill slot"),
    ] {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{ticket_id}-refill.md")),
            format!(
                "# TICKET-{ticket_id}: {title}\n\nGoal: exercise parallel refill scheduling.\nRecommended backend: codex\n"
            ),
        )
        .unwrap();
    }

    // Also force a real maintenance read failure, so this covers the warning
    // path rather than relying only on missing-file short-circuiting.
    let blocked_store = tmp.path().join("quota-store-is-a-directory");
    fs::create_dir(&blocked_store).unwrap();
    let fake_bin = tmp.path().join("bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let slow_release = tmp.path().join("slow-release");
    let slow_started = tmp.path().join("slow-started");
    let call_count_file = tmp.path().join("codex-call-count");
    let active_count_file = tmp.path().join("codex-active-count");
    let active_lock_dir = tmp.path().join("codex-active-count.lock");
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        "#!/bin/sh\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"list\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then printf 'https://github.com/owner/real/pull/1\\n'; exit 0; fi\nexit 0\n",
    );
    // The third worker releases the first, proving refill without a wall-clock
    // deadline. Collect output immediately so a full pipe cannot stall dispatch.
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\n\
             call_count_file='{call_count_file}'\n\
             active_count_file='{active_count_file}'\n\
             active_lock_dir='{active_lock_dir}'\n\
             slow_release='{slow_release}'\n\
             acquire_active_lock() {{ while ! mkdir \"$active_lock_dir\" 2>/dev/null; do sleep 0.01; done; }}\n\
             release_active_lock() {{ rmdir \"$active_lock_dir\"; }}\n\
             inc_active() {{\n\
             \x20\x20acquire_active_lock\n\
             \x20\x20count=$( [ -f \"$active_count_file\" ] && cat \"$active_count_file\" || echo 0 )\n\
             \x20\x20count=$((count + 1))\n\
             \x20\x20echo \"$count\" > \"$active_count_file\"\n\
             \x20\x20release_active_lock\n\
             \x20\x20if [ \"$count\" -gt 2 ]; then echo 'active slot cap exceeded' >&2; exit 97; fi\n\
             }}\n\
             dec_active() {{\n\
             \x20\x20acquire_active_lock\n\
             \x20\x20count=$( [ -f \"$active_count_file\" ] && cat \"$active_count_file\" || echo 0 )\n\
             \x20\x20if [ \"$count\" -gt 0 ]; then count=$((count - 1)); fi\n\
             \x20\x20echo \"$count\" > \"$active_count_file\"\n\
             \x20\x20release_active_lock\n\
             }}\n\
             trap dec_active EXIT INT TERM\n\
             acquire_active_lock\n\
             n=$( [ -f \"$call_count_file\" ] && cat \"$call_count_file\" || echo 0 )\n\
             n=$((n + 1))\n\
             echo \"$n\" > \"$call_count_file\"\n\
             release_active_lock\n\
             inc_active\n\
             printf 'agent edit %s\\n' \"$n\" > \"refill-$n.txt\"\n\
             case \"$n\" in\n\
             \x20\x201) echo started > '{slow_started}'; while [ ! -f \"$slow_release\" ]; do sleep 0.05; done ;;\n\
             \x20\x202) while [ ! -f '{slow_started}' ]; do sleep 0.05; done ;;\n\
             \x20\x203) printf 'release\\n' > \"$slow_release\" ;;\n\
             \x20\x20*) : ;;\n\
             esac\n\
             printf 'codex call %s\\n' \"$n\"\n\
             exit 0\n",
            call_count_file = call_count_file.display(),
            active_count_file = active_count_file.display(),
            active_lock_dir = active_lock_dir.display(),
            slow_release = slow_release.display(),
            slow_started = slow_started.display(),
        ),
    );

    let ledger_path = tmp.path().join("ledger.jsonl");
    let events_path = tmp.path().join("events.jsonl");
    let child = ProcessGroupGuard::new(
        spawn_bin(tmp.path())
            .args([
                "loop",
                "--profile",
                "real",
                "--config-path",
                cfg.to_str().unwrap(),
                "--once",
                "--parallel",
                "2",
            ])
            .env(
                "PATH",
                format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("HOME", &home)
            .env("GITHUB_TOKEN", "token")
            .env("GAH_LEDGER_PATH", &ledger_path)
            .env("GAH_EVENTS_PATH", &events_path)
            .env("GAH_QUOTA_STORE_PATH", &blocked_store)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );

    let output = watchdog::wait_with_output(child);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        slow_release.exists(),
        "the third worker must release its sibling"
    );
    assert_eq!(read_u32_file(&call_count_file), 3);
    assert_eq!(
        String::from_utf8_lossy(&output.stderr)
            .matches("global state retention failed")
            .count(),
        1,
        "maintenance failures warn once and do not prevent dispatch"
    );
}

#[test]
fn parallel_loop_selects_new_review_after_capacity_deferred_sibling() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    let config = fs::read_to_string(&cfg).unwrap().replace(
        "improve_backend = \"codex\"",
        "improve_backend = \"codex\"\nimprove_candidates = [{ backend = \"codex\", model = \"local/test\" }]",
    );
    fs::write(
        &cfg,
        config.replace(
            "[profiles.real.routing]",
            "[profiles.real.max_concurrent_per_model]\n\"codex/local/test\" = 1\n\n[profiles.real.routing]",
        ),
    )
    .unwrap();

    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 701..=702 {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{id}-capacity.md")),
            format!(
                "# TICKET-{id}: Capacity sibling\n\nGoal: exercise refill after route capacity clears.\nRecommended backend: codex\n"
            ),
        )
        .unwrap();
    }

    ProcessCommand::new("git")
        .args(["switch", "-c", "gah/real-review"])
        .current_dir(&repo)
        .output()
        .unwrap();
    fs::write(repo.join("review-change.txt"), "review me\n").unwrap();
    ProcessCommand::new("git")
        .args(["add", "review-change.txt"])
        .current_dir(&repo)
        .output()
        .unwrap();
    ProcessCommand::new("git")
        .args(["commit", "-m", "review fixture"])
        .current_dir(&repo)
        .output()
        .unwrap();
    ProcessCommand::new("git")
        .args(["push", "-u", "origin", "gah/real-review"])
        .current_dir(&repo)
        .env("HOME", &home)
        .output()
        .unwrap();
    ProcessCommand::new("git")
        .args(["switch", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();

    let fake_bin = tmp.path().join("bin");
    let events_path = tmp.path().join("events.jsonl");
    let review_started = tmp.path().join("review-started");
    fs::create_dir_all(&fake_bin).unwrap();
    let pr_updated_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        &format!(
            "#!/bin/sh\n\
             case \"$4\" in\n\
               */pulls\\?*)\n\
                 if grep -q deferred_capacity '{events_path}' 2>/dev/null; then echo '[{{\"title\":\"[GAH] Fix: TICKET-703\",\"body\":\"Review fixture\",\"head\":{{\"ref\":\"gah/real-review\",\"sha\":\"source-sha\"}},\"html_url\":\"https://github.com/owner/real/pull/7\",\"labels\":[],\"number\":7,\"state\":\"open\",\"draft\":true,\"updated_at\":\"{pr_updated_at}\"}}]'; else echo '[]'; fi\n\
                 exit 0 ;;\n\
               */pulls) echo '[{{\"number\":7}}]'; exit 0 ;;\n\
               */check-runs\\?*) echo '{{\"total_count\":1,\"check_runs\":[{{\"status\":\"completed\",\"conclusion\":\"success\"}}]}}'; exit 0 ;;\n\
             esac\n\
             if [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then echo '{{\"number\":7,\"url\":\"https://github.com/owner/real/pull/7\",\"title\":\"[GAH] Fix: TICKET-703\",\"body\":\"Review fixture\",\"headRefName\":\"gah/real-review\",\"baseRefName\":\"main\",\"headRefOid\":\"source-sha\",\"statusCheckRollup\":[{{\"status\":\"COMPLETED\",\"conclusion\":\"SUCCESS\"}}]}}'; exit 0; fi\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then echo 'https://github.com/owner/real/pull/1'; exit 0; fi\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"comment\" ]; then exit 0; fi\n\
             exit 0\n",
            events_path = events_path.display(),
        ),
    );
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        "#!/bin/sh\nsleep 1\nprintf 'implementation complete\n' > implementation.txt\n",
    );
    make_fake_bin_with_body(
        &fake_bin,
        "claude",
        &format!(
            "#!/bin/sh\necho started > '{}'\ncat <<'EOF'\nReview notes\n{{\"verdict\":\"APPROVE\",\"confidence\":\"high\",\"human_required\":false,\"blocking_findings\":[],\"non_blocking_findings\":[],\"risk_notes\":[],\"evidence\":[\"file:review-change.txt\"]}}\nEOF\n",
            review_started.display()
        ),
    );

    let output = spawn_bin(tmp.path())
        .args([
            "loop",
            "--profile",
            "real",
            "--config-path",
            cfg.to_str().unwrap(),
            "--once",
            "--parallel",
            "2",
        ])
        .env(
            "PATH",
            format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("HOME", &home)
        .env("GITHUB_TOKEN", "token")
        .env("GAH_EVENTS_PATH", &events_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("because configured route capacity is busy"),
        "one initial sibling must be capacity-deferred"
    );
    assert!(
        review_started.exists(),
        "newly eligible higher-priority review must launch after the successful sibling"
    );
}

#[test]
fn parallel_loop_reviews_a_finished_ticket_while_a_slow_sibling_still_runs() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");

    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 801..=802 {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{id}-followup.md")),
            format!(
                "# TICKET-{id}: Follow-up in batch\n\nGoal: exercise a review starting mid-batch.\nRecommended backend: codex\n"
            ),
        )
        .unwrap();
    }

    let fake_bin = tmp.path().join("bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let slow_release = tmp.path().join("slow-release");
    let slow_started = tmp.path().join("slow-started");
    let call_count_dir = tmp.path().join("codex-calls");
    let created_pr = tmp.path().join("created-pr");
    let review_started = tmp.path().join("review-started");
    let pr_updated_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    // The first `gh pr create` publishes the fast ticket's PR; from then on
    // the provider lists it as an open draft with passing checks.
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        &format!(
            "#!/bin/sh\n\
             created='{created_pr}'\n\
             case \"$4\" in\n\
               */pulls\\?*)\n\
                 if [ -f \"$created\" ] && [ ! -f '{review_started}' ]; then head=$(sed -n 1p \"$created\"); title=$(sed -n 2p \"$created\"); printf '[{{\"title\":\"%s\",\"body\":\"Follow-up fixture\",\"head\":{{\"ref\":\"%s\",\"sha\":\"source-sha\"}},\"html_url\":\"https://github.com/owner/real/pull/9\",\"labels\":[],\"number\":9,\"state\":\"open\",\"draft\":true,\"updated_at\":\"{pr_updated_at}\"}}]\\n' \"$title\" \"$head\"; else echo '[]'; fi\n\
                 exit 0 ;;\n\
               */pulls) echo '[{{\"number\":9}}]'; exit 0 ;;\n\
               */check-runs\\?*) echo '{{\"total_count\":1,\"check_runs\":[{{\"status\":\"completed\",\"conclusion\":\"success\"}}]}}'; exit 0 ;;\n\
             esac\n\
             if [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then\n\
               head=''; title=''\n\
               while [ $# -gt 0 ]; do case \"$1\" in --head) head=\"$2\" ;; --title) title=\"$2\" ;; esac; shift; done\n\
               if [ ! -f \"$created\" ]; then printf '%s\\n%s\\n' \"$head\" \"$title\" > \"$created\"; fi\n\
               echo 'https://github.com/owner/real/pull/9'; exit 0\n\
             fi\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then head=$(sed -n 1p \"$created\"); title=$(sed -n 2p \"$created\"); printf '{{\"number\":9,\"url\":\"https://github.com/owner/real/pull/9\",\"title\":\"%s\",\"body\":\"Follow-up fixture\",\"headRefName\":\"%s\",\"baseRefName\":\"main\",\"headRefOid\":\"source-sha\",\"statusCheckRollup\":[{{\"status\":\"COMPLETED\",\"conclusion\":\"SUCCESS\"}}]}}\\n' \"$title\" \"$head\"; exit 0; fi\n\
             exit 0\n",
            created_pr = created_pr.display(),
            review_started = review_started.display(),
        ),
    );
    // The first implementation call holds its slot until released; the
    // second finishes at once. `mkdir` makes the call numbering atomic.
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\n\
             mkdir -p '{calls}'\n\
             n=1; while ! mkdir '{calls}'/$n 2>/dev/null; do n=$((n + 1)); done\n\
             printf 'agent edit %s\\n' \"$n\" > \"followup-$n.txt\"\n\
             if [ \"$n\" = 1 ]; then echo started > '{slow_started}'; while [ ! -f '{slow_release}' ]; do sleep 0.05; done; else while [ ! -f '{slow_started}' ]; do sleep 0.05; done; fi\n\
             exit 0\n",
            calls = call_count_dir.display(),
            slow_release = slow_release.display(),
            slow_started = slow_started.display(),
        ),
    );
    make_fake_bin_with_body(
        &fake_bin,
        "claude",
        &format!(
            "#!/bin/sh\necho started > '{}'\nprintf 'release\\n' > '{}'\ncat <<'EOF'\nReview notes\n{{\"verdict\":\"APPROVE\",\"confidence\":\"high\",\"human_required\":false,\"blocking_findings\":[],\"non_blocking_findings\":[],\"risk_notes\":[],\"evidence\":[\"file:followup-2.txt\"]}}\nEOF\n",
            review_started.display(),
            slow_release.display()
        ),
    );

    let child = ProcessGroupGuard::new(
        spawn_bin(tmp.path())
            .args([
                "loop",
                "--profile",
                "real",
                "--config-path",
                cfg.to_str().unwrap(),
                "--once",
                "--parallel",
                "2",
            ])
            .env(
                "PATH",
                format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("HOME", &home)
            .env("GITHUB_TOKEN", "token")
            .env("GAH_LEDGER_PATH", tmp.path().join("ledger.jsonl"))
            .env("GAH_EVENTS_PATH", tmp.path().join("events.jsonl"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );

    // The reviewer releases the slow sibling itself. Draining output while
    // waiting avoids blocking either dispatch on the controller's pipe buffer.
    let output = watchdog::wait_with_output(child);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(review_started.exists(), "the fast ticket must be reviewed");
    assert!(
        slow_release.exists(),
        "the reviewer must release its sibling"
    );
}

#[test]
fn parallel_worker_error_stops_refill_after_running_sibling_finishes() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for (ticket_id, title) in [
        ("411", "Slow sibling"),
        ("412", "Terminal failure"),
        ("413", "Must not refill"),
    ] {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{ticket_id}-bounded.md")),
            format!(
                "# TICKET-{ticket_id}: {title}\n\nGoal: exercise bounded parallel failure handling.\nRecommended backend: codex\n"
            ),
        )
        .unwrap();
    }

    let fake_bin = tmp.path().join("bin");
    let calls = tmp.path().join("calls");
    fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        "#!/bin/sh\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"list\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then printf 'https://github.com/owner/real/pull/1\\n'; exit 0; fi\nexit 0\n",
    );
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\nlock='{calls}.lock'\nwhile ! mkdir \"$lock\" 2>/dev/null; do sleep 0.01; done\nn=$(cat '{calls}.count' 2>/dev/null || echo 0)\nn=$((n + 1))\necho \"$n\" > '{calls}.count'\nif [ \"$n\" -eq 2 ]; then pwd > '{calls}.failure-pwd'; fi\nrmdir \"$lock\"\nif [ \"$n\" -eq 1 ]; then printf 'slow-start\\n' >> '{calls}'; sleep 5; printf 'slow edit\\n' > slow.txt; printf 'slow-done\\n' >> '{calls}'; exit 0; fi\nif [ -f '{calls}.failure-pwd' ] && [ \"$PWD\" = \"$(cat '{calls}.failure-pwd')\" ]; then printf 'failed\\n' >> '{calls}'; exit 9; fi\nprintf 'refilled\\n' >> '{calls}'\nprintf 'unexpected\\n' > third.txt\nexit 0\n",
            calls = calls.display(),
        ),
    );

    let output = spawn_bin(tmp.path())
        .args([
            "loop",
            "--profile",
            "real",
            "--config-path",
            cfg.to_str().unwrap(),
            "--once",
            "--parallel",
            "2",
        ])
        .env(
            "PATH",
            format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("HOME", &home)
        .env("GITHUB_TOKEN", "token")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let calls = fs::read_to_string(calls).unwrap();
    assert!(calls.contains("slow-start"));
    assert!(calls.contains("slow-done"));
    assert!(calls.contains("failed"));
    assert!(!calls.contains("refilled"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("parallel action(s) failed"));
}

#[test]
fn parallel_loop_does_not_refill_after_shutdown() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 501..=503 {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{id}-shutdown.md")),
            format!("# TICKET-{id}: Shutdown refill guard\n\nGoal: prove shutdown stops refill.\nRecommended backend: codex\n"),
        )
        .unwrap();
    }

    let fake_bin = tmp.path().join("bin");
    let calls = tmp.path().join("codex-calls");
    fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        "#!/bin/sh\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"list\" ]; then echo '[]'; fi\nif [ \"$1\" = \"api\" ]; then echo '[]'; fi\nexit 0\n",
    );
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\nprintf 'call\\n' >> '{}'\nwhile :; do sleep 0.05; done\n",
            calls.display()
        ),
    );

    let child = ProcessGroupGuard::new(
        spawn_bin(tmp.path())
            .args([
                "loop",
                "--profile",
                "real",
                "--config-path",
                cfg.to_str().unwrap(),
                "--once",
                "--parallel",
                "2",
            ])
            .env(
                "PATH",
                format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("HOME", &home)
            .env("GITHUB_TOKEN", "token")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );

    let deadline = Instant::now() + Duration::from_secs(20);
    while fs::read_to_string(&calls)
        .map(|text| text.lines().count())
        .unwrap_or(0)
        < 2
    {
        assert!(
            Instant::now() < deadline,
            "two initial workers did not start"
        );
        thread::sleep(Duration::from_millis(20));
    }
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(calls).unwrap().lines().count(), 2);
}

#[test]
fn parallel_loop_reprobes_node_pressure_with_active_worker_remaining() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 601..=602 {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{id}-pressure.md")),
            format!(
                "# TICKET-{id}: Node pressure recovery\n\nGoal: prove refill can happen while active work remains.\nRecommended backend: codex\n"
            ),
        )
        .unwrap();
    }

    let node_pressure = tmp.path().join("node-pressure.json");

    let fake_bin = tmp.path().join("bin");
    let calls = tmp.path().join("codex-calls");
    let active_count = tmp.path().join("codex-active");
    let second_started = tmp.path().join("codex-second-started");
    let active_lock_dir = tmp.path().join("codex-active.lock");
    let slow_release = tmp.path().join("slow-release");
    fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        "#!/bin/sh\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"list\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then printf 'https://github.com/owner/real/pull/1\\n'; exit 0; fi\nexit 0\n",
    );
    // A worker takes its call number and counts itself active under one
    // lock (#1662). With two critical sections, call 2 could count itself
    // and write its marker while call 1 had its number but was not yet
    // counted, so a loaded runner saw one active worker instead of two.
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\nlock='{active_lock_dir}'\n\
             call_count_file='{calls}'\n\
             active_count_file='{active_count}'\n\
             second_started='{second_started}'\n\
             slow_release='{slow_release}'\n\
             acquire_active_lock() {{ while ! mkdir \"$lock\" 2>/dev/null; do sleep 0.01; done; }}\n\
             release_active_lock() {{ rmdir \"$lock\"; }}\n\
             dec_active() {{\n               acquire_active_lock\n\
               count=$( [ -f \"$active_count_file\" ] && cat \"$active_count_file\" || echo 0 )\n\
               if [ \"$count\" -gt 0 ]; then count=$((count - 1)); fi\n\
               echo \"$count\" > \"$active_count_file\"\n\
               release_active_lock\n\
             }}\n\
             acquire_active_lock\n\
             n=$( [ -f \"${{call_count_file}}.count\" ] && cat \"${{call_count_file}}.count\" || echo 0 )\n\
             n=$((n + 1))\n\
             echo \"$n\" > \"${{call_count_file}}.count\"\n\
             count=$( [ -f \"$active_count_file\" ] && cat \"$active_count_file\" || echo 0 )\n\
             echo \"$((count + 1))\" > \"$active_count_file\"\n\
             release_active_lock\n\
             printf 'agent edit %s\n' \"$n\" > \"agent-edit-$n.txt\"\n\
             echo \"call-$n\" >> \"$call_count_file\"\n\
             case \"$n\" in\n\
               1) while [ ! -f \"$slow_release\" ]; do sleep 0.05; done ;;\n\
               2) echo started > \"$second_started\"; while [ ! -f \"$slow_release\" ]; do sleep 0.05; done ;;\n\
             esac\n\
             dec_active\n\
             ",
            active_lock_dir = active_lock_dir.display(),
            calls = calls.display(),
            active_count = active_count.display(),
            second_started = second_started.display(),
            slow_release = slow_release.display()
        ),
    );

    let mut command = spawn_bin(tmp.path());
    write_node_pressure_with_load(&node_pressure, 16, 32, 50.0);
    std::thread::spawn({
        let node_pressure = node_pressure.clone();
        move || {
            std::thread::sleep(Duration::from_millis(500));
            write_node_pressure_with_load(&node_pressure, 16, 32, 0.5);
        }
    });

    let child = ProcessGroupGuard::new(
        command
            .args([
                "loop",
                "--profile",
                "real",
                "--config-path",
                cfg.to_str().unwrap(),
                "--once",
                "--parallel",
                "2",
            ])
            .env(
                "PATH",
                format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("HOME", &home)
            .env("GITHUB_TOKEN", "token")
            .env("GAH_TEST_NODE_PRESSURE_FILE", node_pressure.clone())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );

    // Event-based waits, not wall-clock deadlines (issues #824, #1587): both
    // fake workers block on `slow_release`, so the first stays active until
    // this test releases it and the second worker's marker can only appear
    // while the first is still running. A loaded runner is slow, not wrong;
    // the generous bound only catches a loop that is genuinely stuck.
    let child = wait_for_worker_event(
        child,
        &slow_release,
        || read_u32_file(&active_count) >= 1,
        "first worker did not start",
    );
    let child = wait_for_worker_event(
        child,
        &slow_release,
        || second_started.exists(),
        "second worker did not start while first was active",
    );
    // Neither worker can have finished: both still wait for the release. On a
    // mismatch, release the workers and fail with everything the controller
    // and the fake backend recorded, so the cause is diagnosable (#1662).
    let active = read_u32_file(&active_count);
    fs::write(&slow_release, "release").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        active,
        2,
        "expected both workers active when the second started\ncalls: {}\nstdout: {}\nstderr: {}",
        fs::read_to_string(&calls).unwrap_or_default(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = fs::read_to_string(calls).unwrap();
    assert!(calls.contains("call-2"));
}

#[test]
fn parallel_loop_uses_light_review_after_heavy_node_deferral() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    fs::write(
        repo.join("docs/tickets/TICKET-700-heavy.md"),
        "# TICKET-700: Heavy implementation\n\nGoal: defer under tight memory.\nRecommended backend: codex\n",
    )
    .unwrap();

    ProcessCommand::new("git")
        .args(["switch", "-c", "gah/real-review"])
        .current_dir(&repo)
        .output()
        .unwrap();
    fs::write(repo.join("review-change.txt"), "review me\n").unwrap();
    ProcessCommand::new("git")
        .args(["add", "review-change.txt"])
        .current_dir(&repo)
        .output()
        .unwrap();
    ProcessCommand::new("git")
        .args(["commit", "-m", "review fixture"])
        .current_dir(&repo)
        .output()
        .unwrap();
    ProcessCommand::new("git")
        .args(["push", "-u", "origin", "gah/real-review"])
        .current_dir(&repo)
        .env("HOME", &home)
        .output()
        .unwrap();
    ProcessCommand::new("git")
        .args(["switch", "main"])
        .current_dir(&repo)
        .output()
        .unwrap();

    let fake_bin = tmp.path().join("bin");
    let pulls_count = tmp.path().join("pulls-count");
    let review_started = tmp.path().join("review-started");
    let implementation_started = tmp.path().join("implementation-started");
    fs::create_dir_all(&fake_bin).unwrap();
    // `updated_at` is relative to "now" (not hardcoded) so the fake PR stays
    // classified as active instead of aging into sync::classify's 14-day
    // STALE cutoff over time (same pattern as #801/#802).
    let pr_updated_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        &format!(
            "#!/bin/sh\n\
             case \"$4\" in\n\
               */pulls\\?*)\n\
                 n=$(cat '{pulls_count}' 2>/dev/null || echo 0); n=$((n + 1)); echo \"$n\" > '{pulls_count}'\n\
                 if [ \"$n\" -le 2 ]; then echo '[]'; else echo '[{{\"title\":\"[GAH] Fix: TICKET-701\",\"body\":\"Review fixture\",\"head\":{{\"ref\":\"gah/real-review\",\"sha\":\"source-sha\"}},\"html_url\":\"https://github.com/owner/real/pull/7\",\"labels\":[],\"number\":7,\"state\":\"open\",\"draft\":true,\"updated_at\":\"{pr_updated_at}\"}}]'; fi\n\
                 exit 0 ;;\n\
               */pulls) echo '[{{\"number\":7}}]'; exit 0 ;;\n\
               */check-runs\\?*) echo '{{\"total_count\":1,\"check_runs\":[{{\"status\":\"completed\",\"conclusion\":\"success\"}}]}}'; exit 0 ;;\n\
             esac\n\
             if [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"view\" ]; then echo '{{\"number\":7,\"url\":\"https://github.com/owner/real/pull/7\",\"title\":\"[GAH] Fix: TICKET-701\",\"body\":\"Review fixture\",\"headRefName\":\"gah/real-review\",\"baseRefName\":\"main\",\"headRefOid\":\"source-sha\",\"statusCheckRollup\":[{{\"status\":\"COMPLETED\",\"conclusion\":\"SUCCESS\"}}]}}'; exit 0; fi\n\
             if [ \"$1\" = \"pr\" ] && [ \"$2\" = \"comment\" ]; then exit 0; fi\n\
             exit 0\n",
            pulls_count = pulls_count.display(),
        ),
    );
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\necho started > '{}'\nexit 0\n",
            implementation_started.display()
        ),
    );
    make_fake_bin_with_body(
        &fake_bin,
        "claude",
        &format!(
            "#!/bin/sh\necho started > '{}'\ncat <<'EOF'\nReview notes\n{{\"verdict\":\"APPROVE\",\"confidence\":\"high\",\"human_required\":false,\"blocking_findings\":[],\"non_blocking_findings\":[],\"risk_notes\":[],\"evidence\":[\"file:review-change.txt\"]}}\nEOF\n",
            review_started.display()
        ),
    );

    let node_pressure = tmp.path().join("node-pressure.json");
    let mut command = spawn_bin(tmp.path());
    fs::write(
        &node_pressure,
        r#"{
  "memory_total_bytes": 17179869184,
  "memory_available_bytes": 5368709120,
  "logical_cpus": 32,
  "load_one": 0.5,
  "memory_full_psi_avg10": 0.0,
  "cpu_some_psi_avg10": 0.0
}"#,
    )
    .unwrap();
    let output = command
        .args([
            "loop",
            "--profile",
            "real",
            "--config-path",
            cfg.to_str().unwrap(),
            "--once",
            "--parallel",
            "2",
        ])
        .env(
            "PATH",
            format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("HOME", &home)
        .env("GITHUB_TOKEN", "token")
        .env("GAH_TEST_NODE_PRESSURE_FILE", &node_pressure)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        review_started.exists(),
        "lighter review must launch after heavy work is node-deferred"
    );
    assert!(
        !implementation_started.exists(),
        "heavy implementation backend must not launch through node pressure"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("Deferred dispatch_ticket because node capacity is busy"),
        "heavy deferral must remain observable"
    );
}

#[test]
fn parallel_loop_waits_to_refill_until_node_pressure_recedes() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 603..=604 {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{id}-pressure.md")),
            format!(
                "# TICKET-{id}: Persistent pressure\n\nGoal: prove no refill is launched until pressure normalizes.\nRecommended backend: codex\n"
            ),
        )
        .unwrap();
    }

    let node_pressure = tmp.path().join("node-pressure.json");

    let fake_bin = tmp.path().join("bin");
    let calls = tmp.path().join("codex-calls");
    let provider_calls = tmp.path().join("gh-calls");
    let slow_release = tmp.path().join("slow-release");
    fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"list\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then printf 'https://github.com/owner/real/pull/1\\n'; exit 0; fi\nexit 0\n",
            provider_calls.display()
        ),
    );
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\nn=$( [ -f '{calls}.count' ] && cat '{calls}.count' || echo 0 )\nn=$((n + 1))\necho \"$n\" > '{calls}.count'\nprintf 'agent edit %s\\n' \"$n\" >> 'agent-edits-$n.txt'\nif [ \"$n\" -eq 1 ]; then while [ ! -f '{slow_release}' ]; do sleep 0.05; done; fi\n",
            calls = calls.display(),
            slow_release = slow_release.display()
        ),
    );

    let mut command = spawn_bin(tmp.path());
    write_node_pressure_with_load(&node_pressure, 16, 32, 50.0);
    let child = ProcessGroupGuard::new(
        command
            .args([
                "loop",
                "--profile",
                "real",
                "--config-path",
                cfg.to_str().unwrap(),
                "--once",
                "--parallel",
                "2",
            ])
            .env(
                "PATH",
                format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("HOME", &home)
            .env("GITHUB_TOKEN", "token")
            .env("GAH_TEST_NODE_PRESSURE_FILE", node_pressure.clone())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );

    let start_deadline = Instant::now() + Duration::from_secs(5);
    while read_u32_file(&calls.with_extension("count")) == 0 {
        assert!(
            Instant::now() < start_deadline,
            "first worker did not start"
        );
        thread::sleep(Duration::from_millis(20));
    }

    // Let the initial fill attempt finish, then span several 100 ms capacity
    // re-probes. They must sample only local node state; rebuilding provider
    // snapshots on every tick would create an API request storm.
    thread::sleep(Duration::from_millis(400));
    let provider_calls_before = fs::read_to_string(&provider_calls).unwrap().lines().count();
    thread::sleep(Duration::from_millis(600));
    let provider_calls_after = fs::read_to_string(&provider_calls).unwrap().lines().count();
    assert_eq!(read_u32_file(&calls.with_extension("count")), 1);
    assert_eq!(
        provider_calls_after, provider_calls_before,
        "persistent node pressure must not re-observe provider state"
    );

    let shutdown_start = Instant::now();
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        shutdown_start.elapsed() < Duration::from_secs(5),
        "shutdown should remain prompt while pressure remains deferred"
    );
    assert_eq!(read_u32_file(&calls.with_extension("count")), 1);
}

#[test]
fn parallel_loop_remains_prompt_on_shutdown_while_node_capacity_is_deferred() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for id in 605..=606 {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{id}-pressure-shutdown.md")),
            format!(
                "# TICKET-{id}: Shutdown under deferred capacity\n\nGoal: prove shutdown exits before worker pool completes naturally.\nRecommended backend: codex\n"
            ),
        )
        .unwrap();
    }

    let node_pressure = tmp.path().join("node-pressure.json");

    let fake_bin = tmp.path().join("bin");
    let calls = tmp.path().join("codex-calls");
    fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        "#!/bin/sh\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"list\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then printf 'https://github.com/owner/real/pull/1\\n'; exit 0; fi\nexit 0\n",
    );
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\nn=$( [ -f '{calls}.count' ] && cat '{calls}.count' || echo 0 )\nn=$((n + 1))\necho \"$n\" > '{calls}.count'\nprintf 'agent edit %s\\n' \"$n\" >> 'agent-edits-$n.txt'\nprintf \"call-$n\\n\" >> '{calls}'\nwhile :; do sleep 0.05; done\n",
            calls = calls.display()
        ),
    );

    let mut command = spawn_bin(tmp.path());
    write_node_pressure_with_load(&node_pressure, 16, 32, 50.0);
    let child = ProcessGroupGuard::new(
        command
            .args([
                "loop",
                "--profile",
                "real",
                "--config-path",
                cfg.to_str().unwrap(),
                "--once",
                "--parallel",
                "2",
            ])
            .env(
                "PATH",
                format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("HOME", &home)
            .env("GITHUB_TOKEN", "token")
            .env("GAH_TEST_NODE_PRESSURE_FILE", node_pressure.clone())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );

    let start_deadline = Instant::now() + Duration::from_secs(5);
    while read_u32_file(&calls.with_extension("count")) == 0 {
        assert!(
            Instant::now() < start_deadline,
            "first worker did not start"
        );
        thread::sleep(Duration::from_millis(20));
    }

    let shutdown_start = Instant::now();
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        shutdown_start.elapsed() < Duration::from_secs(5),
        "shutdown should remain prompt while capacity is deferred"
    );
    assert_eq!(read_u32_file(&calls.with_extension("count")), 1);
}

#[test]
fn parallel_loop_reclaims_a_dead_owners_claim_and_releases_only_its_own() {
    let _lock = serial();
    let tmp = test_tempdir();
    let (repo, home, cfg) = setup_fix_dispatch_repo(&tmp, "validation_commands = [\"true\"]\n");
    fs::create_dir_all(repo.join("docs/tickets")).unwrap();
    for (ticket_id, title) in [
        ("421", "Left by a dead loop"),
        ("422", "Held by a live owner"),
    ] {
        fs::write(
            repo.join(format!("docs/tickets/TICKET-{ticket_id}-claims.md")),
            format!(
                "# TICKET-{ticket_id}: {title}\n\nGoal: exercise claim recovery.\nRecommended backend: codex\n"
            ),
        )
        .unwrap();
    }

    // #421 was claimed by a loop that has since died on this host; #422 is
    // held by a live process (this test).
    let dead_pid = {
        let mut child = ProcessCommand::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    };
    let host = hostname::get().unwrap().to_string_lossy().into_owned();
    let claims = tmp.path().join("claims.json");
    fs::write(
        &claims,
        serde_json::json!({
            "version": 2u32,
            "claims": { "real@real": [
                { "work_id": "#421", "pid": dead_pid, "hostname": host,
                  "claimed_at": chrono::Utc::now().to_rfc3339() },
                { "work_id": "#422", "pid": std::process::id(), "hostname": host,
                  "claimed_at": "2026-07-14T00:00:00Z" }
            ]}
        })
        .to_string(),
    )
    .unwrap();

    let fake_bin = tmp.path().join("bin");
    let calls = tmp.path().join("calls");
    fs::create_dir_all(&fake_bin).unwrap();
    make_fake_bin_with_body(
        &fake_bin,
        "gh",
        "#!/bin/sh\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"list\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"api\" ]; then echo '[]'; exit 0; fi\nif [ \"$1\" = \"pr\" ] && [ \"$2\" = \"create\" ]; then printf 'https://github.com/owner/real/pull/1\\n'; exit 0; fi\nexit 0\n",
    );
    make_fake_bin_with_body(
        &fake_bin,
        "codex",
        &format!(
            "#!/bin/sh\nprintf 'call\\n' >> '{calls}'\nprintf 'agent edit\\n' > reclaimed.txt\nexit 0\n",
            calls = calls.display(),
        ),
    );

    let output = spawn_bin(tmp.path())
        .args([
            "loop",
            "--profile",
            "real",
            "--config-path",
            cfg.to_str().unwrap(),
            "--once",
            "--parallel",
            "2",
        ])
        .env(
            "PATH",
            format!("{}:{}", fake_bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("HOME", &home)
        .env("GITHUB_TOKEN", "token")
        .env("GAH_CLAIM_STATE_PATH", &claims)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Only the dead owner's item was dispatched.
    assert_eq!(
        fs::read_to_string(&calls).unwrap_or_default(),
        "call\n",
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    // The loop released its own claim on #421 and left the live owner's.
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&claims).unwrap()).unwrap();
    let left: Vec<_> = state["claims"]["real@real"]
        .as_array()
        .unwrap()
        .iter()
        .map(|claim| (claim["work_id"].clone(), claim["pid"].clone()))
        .collect();
    assert_eq!(
        left,
        [(
            serde_json::json!("#422"),
            serde_json::json!(std::process::id())
        )]
    );
}

#[test]
fn parallel_loop_readmits_the_node_deferred_item_after_pressure_recovers() {
    let _lock = serial();
    node_recovery::readmits_deferred_item(true);
}

#[test]
fn parallel_loop_readmits_the_node_deferred_item_while_a_sibling_is_active() {
    let _lock = serial();
    node_recovery::readmits_deferred_item(false);
}
