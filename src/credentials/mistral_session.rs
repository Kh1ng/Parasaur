//! Short-lived publication tokens, independent of secret values and billing.
//! Source mutations invalidate the existing lock file; no token history grows.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize, PartialEq, Eq)]
enum Version {
    Missing,
    Credential(String),
    Cookie {
        device: u64,
        inode: u64,
        size: u64,
        modified: (i64, i64),
        changed: (i64, i64),
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Publication {
    version: Version,
    token: String,
}

fn token(lock: &mut File, version: Version) -> Result<String> {
    lock.rewind()?;
    let mut bytes = Vec::new();
    lock.take(4097).read_to_end(&mut bytes)?;
    // A partial marker after a crash invalidates earlier checks. Replacing it
    // with a fresh nonce is safe; credential/file validation happens separately.
    if bytes.len() <= 4096 {
        if let Ok(state) = serde_json::from_slice::<Publication>(&bytes) {
            if uuid::Uuid::parse_str(&state.token).is_ok() && state.version == version {
                return Ok(state.token);
            }
        }
    }
    let state = Publication {
        version,
        token: uuid::Uuid::new_v4().to_string(),
    };
    let bytes = serde_json::to_vec(&state)?;
    lock.seek(SeekFrom::Start(0))?;
    lock.set_len(0)?;
    lock.write_all(&bytes)?;
    lock.sync_all()?;
    Ok(state.token)
}

pub(super) fn invalidate(lock: &mut File) -> Result<()> {
    lock.set_len(0)?;
    lock.sync_all()
        .context("cannot invalidate credential publication")
}

pub(super) fn credential_token(root: &Path, id: &str, lock: &mut File) -> Result<String> {
    let version = match std::fs::symlink_metadata(root.join(format!("{id}.json"))) {
        Ok(_) => Version::Credential(super::read_at(root, id)?.revision),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Version::Missing,
        Err(error) => return Err(error).context("credential snapshot unavailable"),
    };
    token(lock, version)
}

#[cfg(unix)]
fn cookie_version(path: &Path) -> Result<Version> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Version::Missing),
        Err(error) => return Err(error).context("Mistral cookie snapshot unavailable"),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        bail!("Mistral cookie must be an owner-only regular file");
    }
    Ok(Version::Cookie {
        device: meta.dev(),
        inode: meta.ino(),
        size: meta.len(),
        modified: (meta.mtime(), meta.mtime_nsec()),
        changed: (meta.ctime(), meta.ctime_nsec()),
    })
}
#[cfg(not(unix))]
fn cookie_version(_path: &Path) -> Result<Version> {
    bail!("Mistral cookies require a Unix execution node")
}

fn destination(root: &Path) -> Result<PathBuf> {
    Ok(root
        .parent()
        .context("Mistral cookie home unavailable")?
        .join("mistral-dashboard.cookie"))
}

fn check_default_source(root: &Path) -> Result<()> {
    let default = destination(root)?;
    if std::env::var_os("MISTRAL_DASHBOARD_COOKIE_FILE")
        .is_some_and(|path| PathBuf::from(path) != default)
    {
        bail!("a custom Mistral cookie source is configured");
    }
    Ok(())
}

pub fn snapshot(id: Option<&str>) -> Result<String> {
    let root = super::root()?;
    if id.is_none() {
        check_default_source(&root)?;
    }
    snapshot_at(&root, id)
}

fn snapshot_at(root: &Path, id: Option<&str>) -> Result<String> {
    if let Some(id) = id {
        let mut lock = super::source_lock(root, id)?;
        credential_token(root, id, &mut lock)
    } else {
        let mut lock = super::source_lock_file(root, ".mistral-dashboard.cookie.lock")?;
        token(&mut lock, cookie_version(&destination(root)?)?)
    }
}

pub fn save(id: Option<&str>, label: Option<&str>, expected: &str, cookie: &str) -> Result<bool> {
    let root = super::root()?;
    if id.is_none() {
        check_default_source(&root)?;
    }
    save_at(
        &root,
        id,
        label,
        expected,
        cookie,
        Some(&crate::quota_store::store_path()),
    )
}

fn save_at(
    root: &Path,
    id: Option<&str>,
    label: Option<&str>,
    expected: &str,
    cookie: &str,
    quota: Option<&Path>,
) -> Result<bool> {
    uuid::Uuid::parse_str(expected).context("invalid credential publication token")?;
    let info = super::CredentialInfo {
        id: id.unwrap_or("default").into(),
        provider: "mistral".into(),
        kind: super::CredentialKind::MistralDashboard,
        account_label: label.unwrap_or("Default").into(),
        env_var: None,
    };
    super::validate(&info)?;
    super::validate_value(&info, cookie)?;
    if id.is_some() {
        if label.is_none() {
            bail!("a named Mistral connection requires an account label");
        }
        return Ok(super::save_if_current_at(root, info, cookie, quota, Some(expected))?.is_some());
    }
    let mut lock = super::source_lock_file(root, ".mistral-dashboard.cookie.lock")?;
    let destination = destination(root)?;
    if token(&mut lock, cookie_version(&destination)?)? != expected {
        return Ok(false);
    }
    let mut file =
        tempfile::NamedTempFile::new_in(root.parent().context("Mistral cookie home unavailable")?)?;
    file.write_all(cookie.as_bytes())?;
    file.as_file().sync_all()?;
    invalidate(&mut lock)?;
    file.persist(destination)
        .map_err(|_| anyhow::anyhow!("cannot save private Mistral cookie"))?;
    Ok(true)
}

#[cfg(test)]
mod tests;
