//! Native AGY metadata, cached per executable and HOME for one minute.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(60);
const RETRY_BACKOFF: Duration = Duration::from_secs(10);
const COLD_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
static WARNED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Bucket {
    pub id: String,
    pub window: String,
    pub remaining_fraction: Option<f64>,
    pub reset_time: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Group {
    #[serde(default)]
    description: String,
    pub buckets: Vec<Bucket>,
}

impl Group {
    /// All windows share capacity. Canonicalize known provider buckets so
    /// probe-free routing and quota observers use the same identity.
    pub fn pool(&self) -> &str {
        pool_identity(
            "",
            &self
                .buckets
                .iter()
                .map(|bucket| bucket.id.as_str())
                .collect::<Vec<_>>(),
        )
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PoolRule {
    id: String,
    model_prefix: String,
    bucket_ids: Vec<String>,
}
#[derive(Deserialize)]
struct PoolPolicy {
    pools: Vec<PoolRule>,
}

/// Interpret the same identity policy as the server observer, with no probe dependency.
fn pool_identity<'a>(model: &str, ids: &[&'a str]) -> &'a str {
    static POLICY: OnceLock<PoolPolicy> = OnceLock::new();
    let policy = POLICY.get_or_init(|| {
        serde_json::from_str(include_str!("../../apps/server/src/agyPoolIdentity.json"))
            .expect("valid shared AGY pool policy")
    });
    let bucket = ids.iter().copied().min();
    let model = model.trim().to_ascii_lowercase();
    policy
        .pools
        .iter()
        .find(|rule| {
            bucket.map_or_else(
                || model.starts_with(&rule.model_prefix),
                |bucket| rule.bucket_ids.iter().any(|id| id == bucket),
            )
        })
        .map(|rule| rule.id.as_str())
        .or(bucket)
        .expect("fallback pool rule")
}

/// Accept only a native usage result, never text returned by a model turn.
pub(crate) fn groups(output: &[u8]) -> Result<Vec<Group>> {
    let root: serde_json::Value = serde_json::from_slice(output)?;
    if root["status"] != "SUCCESS"
        || root["command"]["name"] != "usage"
        || root["num_turns"].as_u64().is_some_and(|turns| turns > 0)
    {
        bail!("Antigravity did not return a usage result");
    }
    let groups: Vec<Group> = serde_json::from_value(root["command"]["data"]["groups"].clone())?;
    if groups.is_empty()
        || groups.iter().any(|group| {
            group.buckets.is_empty()
                || group
                    .buckets
                    .iter()
                    .any(|bucket| !label(&bucket.id) || !label(&bucket.window))
        })
    {
        bail!("Antigravity usage returned invalid quota groups");
    }
    let mut ids = HashSet::new();
    if groups
        .iter()
        .flat_map(|group| &group.buckets)
        .any(|bucket| !ids.insert(&bucket.id))
    {
        bail!("Antigravity usage returned duplicate bucket IDs");
    }
    Ok(groups)
}

fn label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    pools: HashMap<String, String>,
}

impl Catalog {
    /// Membership comes from AGY's explicit family list in each group. Unknown
    /// language or ambiguous membership uses the legacy model-family fallback.
    pub fn parse(models: &str, usage: &[u8]) -> Result<Self> {
        let groups = groups(usage)?;
        let families = groups
            .iter()
            .map(|group| {
                let families = group
                    .description
                    .strip_prefix("Models within this group: ")
                    .context("Antigravity quota group has no model membership")?;
                let names: Vec<_> = families.split(',').map(normalize).collect();
                if names.iter().any(String::is_empty) {
                    bail!("empty Antigravity model family");
                }
                Ok((group.pool(), names))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut pools = HashMap::new();
        for line in models.lines().filter(|line| !line.trim().is_empty()) {
            let Some((id, name)) = line.trim().split_once('\t') else {
                // Some releases send this progress line to stdout.
                if line == "Fetching available models..." {
                    continue;
                }
                bail!("unrecognized Antigravity model list");
            };
            if !label(id) || name.is_empty() {
                bail!("invalid Antigravity model");
            }
            let normalized = normalize(name);
            let matches: Vec<_> = families
                .iter()
                .filter(|(_, names)| {
                    names.iter().any(|family| {
                        normalized == *family || normalized.starts_with(&format!("{family} "))
                    })
                })
                .collect();
            if let [(pool, _)] = matches.as_slice() {
                pools.insert(id.to_string(), (*pool).to_string());
                pools.insert(name.to_string(), (*pool).to_string());
            }
        }
        Ok(Self { pools })
    }

    pub fn pool(&self, account: &str, model: &str) -> String {
        let pool = self
            .pools
            .get(model)
            .map(String::as_str)
            .unwrap_or_else(|| pool_identity(model, &[]));
        format!("{account}:{pool}")
    }
}

// Provider descriptions omit version numbers ("Gemini Flash"). Compare whole
// words after dropping numeric version tokens, never a model-name prefix rule.
fn normalize(name: &str) -> String {
    name.split(|c: char| !c.is_ascii_alphanumeric() && c != '.')
        .filter(|word| !word.is_empty() && !word.chars().all(|c| c.is_ascii_digit() || c == '.'))
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

fn warn() {
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!("warning: unrecognized AGY metadata; using the account-scoped Gemini/third-party quota fallback");
    }
}

type Runtime = (String, Option<PathBuf>);
#[derive(Default)]
struct Cached {
    refreshed_at: Option<Instant>,
    retry_at: Option<Instant>,
    catalog: Catalog,
    refreshing: bool,
    loaded_from_disk: bool,
    has_persisted_catalog: bool,
    cold_probe_attempted: bool,
}
static CACHE: OnceLock<Mutex<HashMap<Runtime, Cached>>> = OnceLock::new();
static REFRESH_THREADS: OnceLock<Mutex<Vec<JoinHandle<()>>>> = OnceLock::new();

fn reap_finished_refreshes() {
    let finished = {
        let mut handles = REFRESH_THREADS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut finished = Vec::new();
        let mut pending = Vec::new();
        for handle in handles.drain(..) {
            if handle.is_finished() {
                finished.push(handle);
            } else {
                pending.push(handle);
            }
        }
        *handles = pending;
        finished
    };
    for handle in finished {
        let _ = handle.join();
    }
}

/// Wait for bounded metadata children before the CLI exits.
pub(crate) fn join_background_refreshes() {
    let handles = std::mem::take(
        &mut *REFRESH_THREADS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    );
    for handle in handles {
        let _ = handle.join();
    }
}

/// Return a persisted inventory immediately and refresh it in the background.
/// On a cold cache, bound the initial probe so one-shot routing can use live
/// membership when available without leaving a metadata child running.
pub(crate) fn catalog(executable: &str, home: Option<&Path>) -> Catalog {
    #[cfg(test)]
    if let Some(catalog) = tests::FIXTURE.with(|cell| cell.borrow().clone()) {
        return catalog;
    }
    reap_finished_refreshes();
    let key = (executable.to_string(), home.map(Path::to_path_buf));
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let cached = cache.entry(key.clone()).or_default();
    if cached.refreshed_at.is_none() && !cached.loaded_from_disk {
        if let Some(catalog) = read_persisted(&key.0, key.1.as_deref()) {
            cached.catalog = catalog;
            cached.has_persisted_catalog = true;
        }
        cached.loaded_from_disk = true;
    }
    if cached.refreshed_at.is_none()
        && !cached.has_persisted_catalog
        && !cached.cold_probe_attempted
        && !cached.refreshing
    {
        cached.refreshing = true;
        cached.cold_probe_attempted = true;
        drop(cache);
        let result = load(&key.0, key.1.as_deref(), COLD_PROBE_TIMEOUT);
        let mut cache = CACHE
            .get()
            .unwrap()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let cached = cache.get_mut(&key).unwrap();
        cached.refreshing = false;
        match result {
            Ok(catalog) => {
                store_catalog(&key.0, key.1.as_deref(), &catalog);
                cached.catalog = catalog;
                cached.refreshed_at = Some(Instant::now());
                cached.retry_at = None;
                WARNED.store(false, Ordering::Relaxed);
            }
            Err(_) => {
                eprintln!("warning: AGY metadata refresh failed; retaining cached quota pools and retrying after 10 seconds");
                cached.retry_at = Some(Instant::now() + RETRY_BACKOFF);
            }
        }
        return cached.catalog.clone();
    }
    let catalog = cached.catalog.clone();
    if !cached.refreshing
        && cached.refreshed_at.is_none_or(|at| at.elapsed() >= TTL)
        && cached.retry_at.is_none_or(|at| Instant::now() >= at)
    {
        cached.refreshing = true;
        let refresh_key = key.clone();
        let spawned = std::thread::Builder::new().name("agy-metadata".into()).spawn(move || {
            let result = load(&refresh_key.0, refresh_key.1.as_deref(), COLD_PROBE_TIMEOUT);
            let mut cache = CACHE.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
            let cached = cache.get_mut(&refresh_key).unwrap();
            cached.refreshing = false;
            match result {
                Ok(catalog) => {
                    if cached.retry_at.take().is_some() {
                        eprintln!("AGY metadata refresh recovered; using the updated quota catalog");
                    }
                    WARNED.store(false, Ordering::Relaxed);
                    store_catalog(&refresh_key.0, refresh_key.1.as_deref(), &catalog);
                    cached.catalog = catalog;
                    cached.refreshed_at = Some(Instant::now());
                }
                Err(_) => {
                    if cached.retry_at.is_none() {
                        eprintln!("warning: AGY metadata refresh failed; retaining cached quota pools and retrying after 10 seconds");
                    }
                    cached.retry_at = Some(Instant::now() + RETRY_BACKOFF);
                }
            }
        });
        match spawned {
            Ok(handle) => REFRESH_THREADS
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(handle),
            Err(_) => {
                let cached = cache.get_mut(&key).unwrap();
                cached.refreshing = false;
                cached.retry_at = Some(Instant::now() + RETRY_BACKOFF);
            }
        }
    }
    catalog
}

pub(crate) fn pool(account: &str, model: &str, executable: &str, home: Option<&Path>) -> String {
    catalog(executable, home).pool(account, model)
}

fn load(executable: &str, home: Option<&Path>, timeout: Duration) -> Result<Catalog> {
    let deadline = Instant::now() + timeout;
    let models = crate::usage::agy::run_until(executable, home, &["models"], deadline)?;
    let help = crate::usage::agy::run_until(executable, home, &["--help"], deadline)?;
    if !String::from_utf8_lossy(&help).contains("--disable-slash-commands") {
        bail!("this Antigravity build does not run slash commands in print mode; usage is not sampled");
    }
    let usage = crate::usage::agy::run_until(
        executable,
        home,
        &["--print", "/usage", "--output-format", "json"],
        deadline,
    )?;
    let parsed = std::str::from_utf8(&models)
        .map_err(anyhow::Error::from)
        .and_then(|models| Catalog::parse(models, &usage));
    if parsed.is_err() {
        warn();
    }
    parsed
}

fn cache_path(executable: &str, home: Option<&Path>) -> Option<PathBuf> {
    let home = home
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))?;
    let root = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache"));
    let key = format!("{}\0{}", executable, home.display());
    let hash = Sha256::digest(key.as_bytes());
    Some(
        root.join("gah")
            .join("agy-catalog")
            .join(format!("{hash:x}.json")),
    )
}

fn read_persisted(executable: &str, home: Option<&Path>) -> Option<Catalog> {
    let path = cache_path(executable, home)?;
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn store_catalog(executable: &str, home: Option<&Path>, catalog: &Catalog) {
    let Some(path) = cache_path(executable, home) else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec(catalog) else {
        return;
    };
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    if std::fs::write(&temp, bytes).is_ok() {
        let _ = std::fs::rename(temp, path);
    }
}

#[cfg(test)]
#[path = "agy_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) use tests::fixture;
