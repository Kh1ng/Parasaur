//! A cancellation targets a live backend invocation, never the loop PID shared by siblings.
use anyhow::{bail, ensure, Context, Result};
use fs2::FileExt;
use serde::Serialize;
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

const READY: &str = "cancel-ready";
const REQUESTED: &str = "cancel-requested";
thread_local! { static SESSION: RefCell<Option<PathBuf>> = const { RefCell::new(None) }; }

pub(crate) struct Scope(Option<PathBuf>);
impl Scope {
    pub(crate) fn enter(session: &Path) -> Self {
        Self(SESSION.with(|current| current.replace(Some(session.to_owned()))))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        SESSION.with(|current| current.replace(self.0.take()));
    }
}

pub(crate) fn requested() -> bool {
    SESSION.with(|current| {
        current
            .borrow()
            .as_ref()
            .is_some_and(|session| session.join(REQUESTED).is_file())
    })
}

#[derive(Debug)]
pub(crate) struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dispatch cancelled by operator")
    }
}
impl std::error::Error for Cancelled {}

pub(crate) fn available(session: &Path) -> bool {
    let Ok(lease) = OpenOptions::new()
        .read(true)
        .write(true)
        .open(session.join(READY))
    else {
        return false;
    };
    lease
        .try_lock_exclusive()
        .is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
}

pub(crate) fn terminal_state() -> &'static str {
    if requested() {
        "cancelled_operator"
    } else {
        "cancelled_shutdown"
    }
}

/// The file is locked for the lifetime of the invocation. Renaming READY wins
/// against its removal on normal completion, including at the final poll.
pub(crate) struct Invocation {
    path: PathBuf,
    _lease: File,
}
impl Invocation {
    pub(crate) fn arm() -> Option<Self> {
        SESSION.with(|current| {
            let path = current.borrow().as_ref()?.join(READY);
            let staged = tempfile::NamedTempFile::new_in(path.parent()?).ok()?;
            staged.as_file().try_lock_exclusive().ok()?;
            let lease = staged.persist_noclobber(&path).ok()?;
            Some(Self {
                path,
                _lease: lease,
            })
        })
    }
}
impl Drop for Invocation {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Serialize, serde::Deserialize)]
pub(crate) struct Receipt {
    pub profile: String,
    pub work_id: String,
    pub run_id: String,
}

pub(crate) fn request(
    cfg: &crate::config::GahConfig,
    work_id: &str,
    profile: Option<&str>,
    run_id: Option<&str>,
    local: bool,
) -> Result<Receipt> {
    if !local {
        let role = crate::node_role::NodeRoleStatus::resolve(&cfg.defaults)?;
        if role.role == crate::node_role::NodeRole::Central {
            let configured = std::env::var("PARASAUR_SERVER_URL")
                .ok()
                .or(role.central_url);
            let default_origin = crate::mcp::DEFAULT_SERVER_URL;
            // A fresh CLI-only installation also defaults to the central role.
            // Probe the default loopback listener before sending any mutation.
            let local_server = || -> Option<String> {
                let addr = url::Url::parse(default_origin)
                    .ok()?
                    .socket_addrs(|| None)
                    .ok()?
                    .into_iter()
                    .next()?;
                std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(250))
                    .ok()?;
                Some(default_origin.to_owned())
            };
            if let Some(origin) = configured.or_else(local_server) {
                return request_central(&origin, work_id, profile, run_id);
            }
        }
    }
    let wanted = crate::work_claim::normalize_work_identity(work_id);
    ensure!(!wanted.is_empty(), "A work ID is required");
    if let Some(name) = profile {
        crate::config::get_profile(cfg, name)?;
    }
    let mut matches = Vec::new();
    for (name, project) in &cfg.profiles {
        if profile.is_some_and(|selected| selected != name) {
            continue;
        }
        let root = Path::new(&project.artifact_root);
        for row in crate::running_workers::observe(root, crate::clock::now_utc()) {
            if !row.cancellable
                || &row.profile != name
                || run_id.is_some_and(|id| id != row.run_id)
                || row
                    .work_id
                    .as_deref()
                    .map(crate::work_claim::normalize_work_identity)
                    .as_deref()
                    != Some(&wanted)
                || uuid::Uuid::parse_str(&row.run_id).is_err()
            {
                continue;
            }
            let session = root.join("sessions").join(&row.run_id);
            matches.push((
                session,
                Receipt {
                    profile: name.clone(),
                    work_id: row.work_id.unwrap(),
                    run_id: row.run_id,
                },
            ));
        }
    }
    ensure!(
        matches.len() <= 1,
        "Multiple running dispatches match {work_id}; specify --profile and --run-id"
    );
    let Some((session, receipt)) = matches.pop() else {
        bail!("No cancellable running dispatch matches {work_id}")
    };
    fs::rename(session.join(READY), session.join(REQUESTED))
        .context("Dispatch completed or cancellation was already requested")?;
    Ok(receipt)
}

fn request_central(
    origin: &str,
    work_id: &str,
    profile: Option<&str>,
    run_id: Option<&str>,
) -> Result<Receipt> {
    let mut url = url::Url::parse(origin)?;
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "Invalid central origin"
    );
    let loopback = url.host().is_some_and(|host| match host {
        url::Host::Domain(name) => name == "localhost",
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    });
    ensure!(url.scheme() == "https" || (url.scheme() == "http" &&
        (loopback || std::env::var("PARASAUR_ALLOW_INSECURE_HTTP").as_deref() == Ok("1"))),
        "Use an HTTPS central origin or set PARASAUR_ALLOW_INSECURE_HTTP=1 for a trusted HTTP connection");
    url.set_path("/api/dispatch/cancel");
    let token = std::env::var("PARASAUR_SERVER_TOKEN")
        .or_else(|_| std::env::var("COORDINATOR_TOKEN"))
        .ok();
    let body = serde_json::json!({"work_id":work_id,"profile":profile,"run_id":run_id});
    let reply = crate::curl_http::request_with_idempotency_key(
        "POST",
        url.as_str(),
        Some(&body.to_string()),
        token.as_deref(),
        30,
        Some(&uuid::Uuid::new_v4().to_string()),
    )?;
    if reply.status != 202 {
        let message = serde_json::from_slice::<serde_json::Value>(&reply.body)
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_owned));
        bail!(
            "Central cancellation returned {}: {}",
            reply.status,
            message.as_deref().unwrap_or("no confirmation")
        );
    }
    let receipt: Receipt = serde_json::from_slice(&reply.body)?;
    ensure!(
        crate::work_claim::normalize_work_identity(&receipt.work_id)
            == crate::work_claim::normalize_work_identity(work_id)
            && profile.is_none_or(|p| p == receipt.profile)
            && run_id.is_none_or(|r| r == receipt.run_id),
        "Central returned a different dispatch"
    );
    Ok(receipt)
}
