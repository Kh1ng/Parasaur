//! A private, atomic node update ledger, separate from GAH's restart ledger.
use anyhow::{Context, Result};
use fs2::FileExt;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

fn directory() -> Result<PathBuf> {
    let root = match std::env::var_os("XDG_STATE_HOME") {
        Some(path) => PathBuf::from(path),
        None => {
            PathBuf::from(std::env::var_os("HOME").context("HOME is unset")?).join(".local/state")
        }
    };
    let dir = root.join("gah/agent-cli-updates");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn lock_file() -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(directory()?.join("update.lock"))?)
}

pub(crate) fn lock() -> Result<File> {
    let file = lock_file()?;
    file.try_lock_exclusive()
        .context("Another node or agent CLI update is running")?;
    Ok(file)
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Running,
    Success,
    Failed,
}

#[derive(Serialize)]
pub(super) struct UpdateRecord {
    backend: &'static str,
    command: String,
    pub(super) before: Option<String>,
    after: Option<String>,
    status: Status,
    pid: u32,
    started_at: String,
    finished_at: Option<String>,
    error: Option<String>,
}

impl UpdateRecord {
    pub(super) fn new(backend: &'static str, command: String) -> Self {
        Self {
            backend,
            command,
            before: None,
            after: None,
            status: Status::Running,
            pid: std::process::id(),
            started_at: chrono::Utc::now().to_rfc3339(),
            finished_at: None,
            error: None,
        }
    }
    pub(super) fn finish(&mut self, result: &Result<String>) {
        self.finished_at = Some(chrono::Utc::now().to_rfc3339());
        match result {
            Ok(version) => {
                self.status = Status::Success;
                self.after = Some(version.clone());
            }
            Err(e) => {
                self.status = Status::Failed;
                self.error = Some(e.to_string());
            }
        }
    }
}

pub(super) fn save(record: &UpdateRecord) -> Result<()> {
    let dir = directory()?;
    let mut file = tempfile::NamedTempFile::new_in(&dir)?;
    serde_json::to_writer(&mut file, record)?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(dir.join(format!("{}.json", record.backend)))?;
    Ok(())
}

/// Hold while admitting a dispatch. Maintenance takes the exclusive side,
/// then checks durable claims; neither side can pass the other unnoticed.
pub(crate) fn dispatch_admission() -> Result<Option<File>> {
    let file = lock_file()?;
    match FileExt::try_lock_shared(&file) {
        Ok(()) => Ok(Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(error).context("Checking node maintenance admission"),
    }
}
