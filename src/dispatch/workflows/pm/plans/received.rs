//! Install only the artifact belonging to an accepted native receipt. A lost
//! acknowledgement can replay the same file, including after a process crash.
pub(crate) use super::super::publish::MAX_ARTIFACT_BYTES;
use super::super::publish::{parse_artifact, parse_issue_number, plan_fingerprint};
use crate::config::Profile;
use anyhow::{ensure, Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;

pub(crate) fn ensure_shareable(value: &serde_json::Value) -> Result<()> {
    let mut clean = value.clone();
    crate::redact::redact_json_value(&mut clean);
    ensure!(
        &clean == value,
        "Native PM artifact contains secret-bearing content"
    );
    Ok(())
}

pub(crate) fn validate(
    profile: &Profile,
    work: &str,
    fingerprint: Option<&str>,
    text: &str,
) -> Result<()> {
    let artifact = parse_artifact(text.as_bytes(), profile)?;
    ensure!(
        work == format!("#{}", parse_issue_number(&artifact.target)?)
            && fingerprint == Some(&plan_fingerprint(&artifact)?),
        "Returned PM plan differs from its issue or validated fingerprint"
    );
    let original: serde_json::Value = serde_json::from_str(text)?;
    ensure_shareable(&original)?;
    ensure!(
        original == serde_json::to_value(artifact)?,
        "Returned PM plan contains unexpected fields"
    );
    Ok(())
}

fn directory(path: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {
            File::open(path.parent().context("Missing plan directory parent")?)?.sync_all()?
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_dir(),
        "Invalid PM session directory"
    );
    Ok(())
}

pub(crate) fn install(profile: &Profile, run_id: &str, text: &str) -> Result<()> {
    super::validate_id(run_id)?;
    fs::create_dir_all(&profile.artifact_root)?;
    let root = fs::canonicalize(&profile.artifact_root)?;
    let sessions = root.join("sessions");
    directory(&sessions)?;
    let session = sessions.join(run_id);
    directory(&session)?;
    let path = session.join("pm-plan-v1.json");
    let mut file = tempfile::NamedTempFile::new_in(&session)?;
    file.write_all(text.as_bytes())?;
    file.as_file().sync_all()?;
    match file.persist_noclobber(&path) {
        Ok(_) => {}
        Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&path)?;
            ensure!(
                existing.metadata()?.is_file(),
                "Existing PM artifact must be a regular file"
            );
            let mut bytes = Vec::new();
            (&existing)
                .take(MAX_ARTIFACT_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes == text.as_bytes(),
                "A different PM artifact already owns this session"
            );
            existing.sync_all()?;
        }
        Err(error) => return Err(error.error.into()),
    }
    File::open(&session)?.sync_all()?;
    // Reconfirm all directory entries even when a previous create succeeded
    // but its parent sync failed before acknowledgement.
    File::open(&sessions)?.sync_all()?;
    File::open(&root)?.sync_all()?;
    Ok(())
}
