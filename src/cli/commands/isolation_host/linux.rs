use super::Args;
use crate::controller::{acquire_node_capacity, NextAction, NodeCapacityAdmission};
use anyhow::{bail, ensure, Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{
    fs::MetadataExt,
    process::{CommandExt, ExitStatusExt},
};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::time::{Duration, Instant};

fn systemctl(arguments: &[&str]) -> Result<String> {
    let mut command = Command::new("/usr/bin/systemctl");
    command
        .args(["--user"])
        .args(arguments)
        .stdin(Stdio::null());
    let output = crate::runner::process::run_bounded(command, Duration::from_secs(2))
        .context("User systemd manager did not respond.")?;
    ensure!(
        output.status.success() && output.stdout.len() <= 16384,
        "User systemd state unavailable."
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn capacity_config(path: &Path, memory_mib: u64) -> Result<crate::config::NodeCapacitySettings> {
    ensure!(
        path.is_absolute(),
        "Use an absolute private host configuration path."
    );
    let stat =
        std::fs::symlink_metadata(path).context("Host capacity configuration unavailable.")?;
    ensure!(
        stat.is_file()
            && stat.uid() == unsafe { libc::geteuid() }
            && stat.mode() & 0o077 == 0
            && stat.len() <= 1048576,
        "Host capacity configuration must be a private, bounded regular file."
    );
    let text = std::fs::read_to_string(path).context("Host capacity configuration unavailable.")?;
    let cfg: crate::config::GahConfig = toml::from_str(&text)
        .map_err(|_| anyhow::anyhow!("Invalid host capacity configuration."))?;
    let mut settings = cfg.defaults.node_capacity;
    settings.worker_memory_mib = memory_mib.max(512);
    settings.validate()?;
    Ok(settings)
}

fn trusted_executable(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "Use absolute host executable paths.");
    let stat = std::fs::metadata(path)?;
    ensure!(
        stat.is_file() && stat.mode() & 0o6022 == 0,
        "Unsafe isolation executable permissions."
    );
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
    // Privileged exec clears PR_SET_PDEATHSIG; xattr presence is refused even
    // when those capabilities would not grant privileges to this user.
    let result = unsafe {
        libc::getxattr(
            path.as_ptr(),
            c"security.capability".as_ptr(),
            std::ptr::null_mut(),
            0,
        )
    };
    let error = std::io::Error::last_os_error();
    ensure!(
        result < 0 && matches!(error.raw_os_error(), Some(libc::ENODATA | libc::EOPNOTSUPP)),
        "Privileged isolation executables are unsupported."
    );
    Ok(())
}

fn receipt(
    channel: &mut File,
    state: &str,
    unit: &str,
    reason: Option<&str>,
) -> std::io::Result<()> {
    writeln!(
        channel,
        "{}",
        serde_json::json!({"schema":"gah.isolation-host.v2","state":state,"unit":unit,"reason":reason})
    )?;
    channel.flush()
}

fn cgroup_empty(group: &Path) -> bool {
    match std::fs::read_to_string(group.join("cgroup.events")) {
        Ok(text) => text.lines().any(|line| line == "populated 0"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            matches!(group.try_exists(), Ok(false))
        }
        Err(_) => false,
    }
}

/// Keep the existing host lease until the isolated cgroup is empty. The same
/// locked open file description is retained by Bubblewrap's --sync-fd monitor,
/// so an abrupt broker death cannot strand a live job without its reservation.
pub(super) fn run(args: Args) -> Result<()> {
    crate::runner::process::install_shutdown_handler()?;
    trusted_executable(&args.bubblewrap)?;
    trusted_executable(Path::new("/usr/bin/systemd-run"))?;
    ensure!(
        !args.command.is_empty() && Path::new(&args.command[0]).is_absolute(),
        "Invalid isolated command."
    );
    let unit = format!("parasaur-job-{}.scope", args.execution_id);
    let base = systemctl(&[
        "show",
        "--property=ControlGroup",
        "--value",
        "--",
        "-.slice",
    ])?;
    ensure!(
        base.starts_with("/user.slice/") && !base.contains(".."),
        "Unsupported user cgroup delegation."
    );
    let group = PathBuf::from("/sys/fs/cgroup")
        .join(base.trim_start_matches('/'))
        .join("parasaur.slice")
        .join("parasaur-jobs.slice")
        .join(&unit);
    ensure!(
        cgroup_empty(&group),
        "Execution scope requires reconciliation before admission."
    );
    let settings = capacity_config(&args.config, args.memory_mib)?;
    ensure!(
        unsafe { libc::fcntl(5, libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
        "Isolation receipt channel unavailable."
    );
    // SAFETY: fd 5 is the caller-owned private receipt channel in this protocol,
    // validated above before taking ownership of it.
    let mut channel = unsafe { File::from_raw_fd(5) };
    let (send, receive) = sync_channel(1);
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        loop {
            let mut byte = [0];
            if stdin.read_exact(&mut byte).is_err() {
                let _ = send.send(None);
                break;
            }
            if send.send(Some(byte[0])).is_err() {
                break;
            }
        }
    });
    // The connector flushes this process's identity before sending 'g'. Until
    // then, owner death or timeout cannot leave a late scope/lease creation.
    receipt(&mut channel, "ready", &unit, None)?;
    if !matches!(
        receive.recv_timeout(Duration::from_secs(args.heartbeat_seconds)),
        Ok(Some(b'g'))
    ) {
        return Ok(());
    }
    let lease = match acquire_node_capacity(
        &NextAction::DispatchTicket {
            ticket_path: String::new(),
            work_id: None,
            recommended_backend: None,
            recommended_model: None,
            reason: "Isolated host reservation".into(),
        },
        0,
        settings,
    )? {
        NodeCapacityAdmission::Admit(lease) => lease,
        NodeCapacityAdmission::Defer(reason) => {
            receipt(&mut channel, "deferred", &unit, Some(&reason))?;
            return Ok(());
        }
    };
    let monitor = lease.monitor_file()?;
    let monitor_fd = monitor.as_raw_fd();
    // Use a separate descriptor for the lease even if this process opened fd 6
    // for another file first. The receipt channel must never reach the job.
    let parent = unsafe { libc::getpid() };
    let mut command = Command::new("/usr/bin/systemd-run");
    command
        .args([
            "--user",
            "--scope",
            "--quiet",
            "--collect",
            "--slice=parasaur-jobs.slice",
        ])
        .arg(format!("--unit={unit}"))
        .arg(format!(
            "--property=MemoryMax={}",
            args.memory_mib * 1048576
        ))
        .args([
            "--property=MemorySwapMax=0",
            "--property=TimeoutStopSec=2",
            "--property=KillMode=control-group",
        ])
        .arg(format!("--property=TasksMax={}", args.max_processes))
        .arg(format!("--property=RuntimeMaxSec={}", args.timeout_seconds))
        .arg(&args.bubblewrap)
        .args(["--sync-fd", "6", "--args", "3", "--"])
        .args(&args.command)
        .stdin(Stdio::piped());
    // SAFETY: only async-signal-safe syscalls run in the fork child. Check the
    // original parent on both sides of registration to reject startup races.
    unsafe {
        command.pre_exec(move || {
            if libc::getppid() != parent
                || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0
                || libc::getppid() != parent
            {
                return Err(std::io::Error::other("Isolation host owner unavailable."));
            }
            if libc::dup2(monitor_fd, 6) < 0 || libc::fcntl(6, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().context("Starting isolated scope.")?;
    let mut approval = child
        .stdin
        .take()
        .expect("piped child stdin is owned by the host");
    let mut deadline = Instant::now() + Duration::from_secs(args.heartbeat_seconds);
    let hard_deadline = Instant::now() + Duration::from_secs(args.timeout_seconds);
    let mut approved = false;
    let mut stopping = receipt(&mut channel, "admitted", &unit, None).is_err();
    let mut stop_reason = if stopping { "owner_closed" } else { "exited" };
    let mut child_status = None;
    loop {
        if child_status.is_none() {
            match child.try_wait() {
                Ok(status) => child_status = status,
                Err(_) => stopping = true,
            }
        }
        if !stopping {
            if child_status.is_some() {
                stopping = true;
            } else if Instant::now() >= deadline {
                stopping = true;
                stop_reason = "heartbeat_expired";
            } else if Instant::now() >= hard_deadline {
                stopping = true;
                stop_reason = "deadline";
            } else if crate::runner::shutdown_requested() {
                stopping = true;
                stop_reason = "shutdown";
            }
        }
        if stopping {
            // cgroup.kill is an independent kernel path even when the user's
            // systemd manager is frozen or cannot answer D-Bus requests.
            if !cgroup_empty(&group) {
                if let Ok(mut file) = OpenOptions::new()
                    .write(true)
                    .open(group.join("cgroup.kill"))
                {
                    let _ = file.write_all(b"1");
                }
            }
            let _ = child.kill();
            if child_status.is_none() {
                if let Ok(status) = child.try_wait() {
                    child_status = status;
                }
            }
            if child_status.is_some() && cgroup_empty(&group) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        match receive.recv_timeout(Duration::from_millis(50)) {
            Ok(Some(b'h')) => {
                deadline = Instant::now() + Duration::from_secs(args.heartbeat_seconds);
                stopping = receipt(&mut channel, "renewed", &unit, None).is_err();
                if stopping {
                    stop_reason = "owner_closed";
                }
            }
            Ok(Some(b'1')) if !approved => {
                approved = true;
                stopping = approval
                    .write_all(b"1")
                    .and_then(|_| approval.flush())
                    .is_err();
                if stopping {
                    stop_reason = "approval_failed";
                }
            }
            Ok(None) | Err(RecvTimeoutError::Disconnected) => {
                stopping = true;
                stop_reason = "owner_closed";
            }
            Ok(Some(_)) => {
                stopping = true;
                stop_reason = "protocol_refused";
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    drop(approval);
    drop(monitor);
    drop(lease);
    let code = child_status.and_then(|status| status.code()).unwrap_or(125);
    let _ = writeln!(
        channel,
        "{}",
        serde_json::json!({"schema":"gah.isolation-host.v2","state":"stopped","unit":unit,
        "exitCode":child_status.and_then(|status| status.code()),"signal":child_status.and_then(|status| status.signal()),"reason":stop_reason})
    );
    if code != 0 {
        bail!("Isolated scope exited with status {code}.");
    }
    Ok(())
}
