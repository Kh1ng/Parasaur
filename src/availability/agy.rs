//! Native AGY metadata, cached per executable and HOME for one minute.
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(60);
const RETRY_BACKOFF: Duration = Duration::from_secs(10);
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

#[derive(Clone, Default)]
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
}
static CACHE: OnceLock<Mutex<HashMap<Runtime, Cached>>> = OnceLock::new();

/// Return the last good inventory immediately and start at most one background
/// refresh per installation. Probes never hold the cache lock or block routing.
pub(crate) fn catalog(executable: &str, home: Option<&Path>) -> Catalog {
    #[cfg(test)]
    if let Some(catalog) = tests::FIXTURE.with(|cell| cell.borrow().clone()) {
        return catalog;
    }
    let key = (executable.to_string(), home.map(Path::to_path_buf));
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let cached = cache.entry(key.clone()).or_default();
    let catalog = cached.catalog.clone();
    if !cached.refreshing
        && cached.refreshed_at.is_none_or(|at| at.elapsed() >= TTL)
        && cached.retry_at.is_none_or(|at| Instant::now() >= at)
    {
        cached.refreshing = true;
        let refresh_key = key.clone();
        let spawned = std::thread::Builder::new().name("agy-metadata".into()).spawn(move || {
            let result = load(&refresh_key.0, refresh_key.1.as_deref());
            let mut cache = CACHE.get().unwrap().lock().unwrap_or_else(|e| e.into_inner());
            let cached = cache.get_mut(&refresh_key).unwrap();
            cached.refreshing = false;
            match result {
                Ok(catalog) => {
                    if cached.retry_at.take().is_some() {
                        eprintln!("AGY metadata refresh recovered; using the updated quota catalog");
                    }
                    WARNED.store(false, Ordering::Relaxed);
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
        if spawned.is_err() {
            let cached = cache.get_mut(&key).unwrap();
            cached.refreshing = false;
            cached.retry_at = Some(Instant::now() + RETRY_BACKOFF);
        }
    }
    catalog
}

pub(crate) fn pool(account: &str, model: &str, executable: &str, home: Option<&Path>) -> String {
    catalog(executable, home).pool(account, model)
}

fn load(executable: &str, home: Option<&Path>) -> Result<Catalog> {
    let models = crate::usage::agy::run(executable, home, &["models"], 10)?;
    let usage = crate::usage::agy::read_usage(executable, home)?;
    let parsed = std::str::from_utf8(&models)
        .map_err(anyhow::Error::from)
        .and_then(|models| Catalog::parse(models, &usage));
    if parsed.is_err() {
        warn();
    }
    parsed
}

#[cfg(test)]
#[path = "agy_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) use tests::fixture;
