//! Release only the recorded holder and verify its actual kernel lease. The
//! verifier descriptor stays locked until the caller persists Released.
use super::*;
use crate::ledger::remote_dispatch::DispatchBinding;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Lease {
    version: u32,
    binding: DispatchBinding,
    holder_invocation: String,
    path: PathBuf,
    device: u64,
    inode: u64,
}

fn registry() -> Result<PathBuf> {
    let runtime = PathBuf::from(
        std::env::var_os("XDG_RUNTIME_DIR").context("Native capacity runtime is unavailable")?,
    );
    super::super::private_directory(&runtime)?;
    let path = runtime.join("gah-node-capacity");
    // Ordinary capacity workers use their normal umask here. The enclosing
    // owned 0700 runtime directory supplies the privacy boundary.
    let metadata = std::fs::symlink_metadata(&path)?;
    ensure!(
        metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() },
        "Native capacity registry is not an owned directory"
    );
    Ok(path)
}

pub(in crate::native_handoff) fn record_lease(
    context: &WorkerContext,
    work: &Path,
    holder: &str,
    file: &File,
) -> Result<()> {
    let path = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))?;
    ensure!(
        path.parent() == Some(registry()?.as_path())
            && path.extension().is_some_and(|value| value == "lease"),
        "Native lease is outside its owned registry"
    );
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.nlink() == 1 && metadata.uid() == unsafe { libc::geteuid() },
        "Native lease is not an owned registry file"
    );
    let lease = Lease {
        version: 1,
        binding: context.binding.clone(),
        holder_invocation: holder.into(),
        path,
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    super::super::write_blob(work, "capacity.json", &serde_json::to_vec(&lease)?)?;
    File::open(work)?.sync_all()?;
    Ok(())
}

fn group_empty(run_id: &str) -> Result<()> {
    let values = state("app.slice")?;
    let parent = values
        .get("ControlGroup")
        .filter(|value| {
            value.starts_with('/')
                && value.ends_with("/app.slice")
                && !value.split('/').any(|part| part == "..")
                && value
                    .split('/')
                    .any(|part| part == format!("user@{}.service", unsafe { libc::geteuid() }))
        })
        .context("Native cgroup parent is unavailable")?;
    let directory = Path::new("/sys/fs/cgroup")
        .join(parent.trim_start_matches('/'))
        .join(format!("gah-native-job-{run_id}.service"));
    match std::fs::read_to_string(directory.join("cgroup.events")) {
        Ok(value) => ensure!(
            value.lines().any(|line| line == "populated 0"),
            "Native job descendants remain alive"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !directory.exists() => {}
        Err(_) => anyhow::bail!("Native job cgroup state is unavailable"),
    }
    Ok(())
}

fn job_empty(run_id: &str, identity: &JobIdentity) -> Result<()> {
    let unit = format!("gah-native-job-{run_id}.service");
    let values = state(&unit)?;
    if values.get("LoadState").map(String::as_str) == Some("not-found") {
        ensure!(
            values.get("MainPID").map(String::as_str) == Some("0")
                && values.get("InvocationID").is_some_and(String::is_empty),
            "Native absent job has inconsistent identity"
        );
    } else {
        terminal_state(&values, &unit, identity)?;
    }
    group_empty(run_id)
}

fn holder_absent(run_id: &str, holder: &str) -> Result<bool> {
    let unit = format!("gah-native-capacity-{run_id}.service");
    let values = state(&unit)?;
    let empty = values.get("MainPID").map(String::as_str) == Some("0")
        && values.get("NFileDescriptorStore").map(String::as_str) == Some("0");
    if empty
        && values.get("ActiveState").map(String::as_str) == Some("inactive")
        && values.get("SubState").map(String::as_str) == Some("dead")
        && values.get("InvocationID").map(String::as_str) == Some(holder)
    {
        validate_contract(&values, &unit, holder)?;
        return Ok(true);
    }
    Ok(empty
        && values.get("LoadState").map(String::as_str) == Some("not-found")
        && values.get("MainPID").map(String::as_str) == Some("0")
        && values.get("NFileDescriptorStore").map(String::as_str) == Some("0")
        && values.get("InvocationID").is_some_and(String::is_empty))
}

/// Caller must persist a verified acknowledgement and StopIntent first. A
/// changed holder/job is never stopped, even on a cold retry after lost replies.
pub(in crate::native_handoff) fn release_capacity(
    binding: &DispatchBinding,
    holder: &str,
    identity: &JobIdentity,
    work: &Path,
) -> Result<Option<File>> {
    capacity_state(binding, holder, identity, work, true)
}

pub(in crate::native_handoff) fn released_capacity(
    binding: &DispatchBinding,
    holder: &str,
    identity: &JobIdentity,
    work: &Path,
) -> Result<()> {
    drop(capacity_state(binding, holder, identity, work, false)?);
    Ok(())
}

fn capacity_state(
    binding: &DispatchBinding,
    holder: &str,
    identity: &JobIdentity,
    work: &Path,
    stop: bool,
) -> Result<Option<File>> {
    let lease: Lease = super::super::read_json(&super::super::read_blob(
        &work.join("capacity.json"),
        None,
        super::super::MAX_JSON,
    )?)?;
    ensure!(
        lease.version == 1
            && serde_json::to_vec(&lease.binding)? == serde_json::to_vec(binding)?
            && lease.holder_invocation == holder
            && lease.path.parent() == Some(registry()?.as_path())
            && lease.path.extension().is_some_and(|value| value == "lease"),
        "Native capacity identity changed"
    );
    job_empty(&binding.run_id, identity)?;
    let absent = holder_absent(&binding.run_id, holder)?;
    let verifier = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&lease.path)
    {
        Ok(file) => {
            let metadata = file.metadata()?;
            ensure!(
                metadata.is_file()
                    && metadata.dev() == lease.device
                    && metadata.ino() == lease.inode
                    && metadata.uid() == unsafe { libc::geteuid() }
                    && metadata.nlink() == 1,
                "Native capacity lease was replaced"
            );
            Some(file)
        }
        // The ordinary registry scanner removes entries only after obtaining
        // their free kernel lock. A stopped holder plus collected entry is a
        // valid cold retry; never unlink a live lease ourselves.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && absent => None,
        Err(_) => anyhow::bail!("Native capacity lease is unavailable"),
    };
    if !absent {
        ensure!(stop, "Native capacity release is not currently confirmed");
        require_ready(&binding.run_id, holder)?;
        let mut command = Command::new("/usr/bin/systemctl");
        command.args([
            "--user",
            "stop",
            &format!("gah-native-capacity-{}.service", binding.run_id),
        ]);
        let response = crate::runner::process::run_bounded(command, Duration::from_secs(3))
            .context("Native capacity stop reply is unconfirmed; reconcile it")?;
        ensure!(
            response.status.success(),
            "Native capacity stop was not confirmed"
        );
    }
    ensure!(
        holder_absent(&binding.run_id, holder)?,
        "Native capacity holder remains or changed"
    );
    job_empty(&binding.run_id, identity)?;
    if let Some(file) = &verifier {
        file.try_lock_exclusive()
            .context("Native kernel capacity lease remains held")?;
    }
    Ok(verifier)
}
