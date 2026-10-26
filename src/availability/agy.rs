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
    /// All windows in a reported group share capacity. Use a provider bucket
    /// ID rather than a GAH model-family table, independent of response order.
    pub fn pool(&self) -> &str {
        self.buckets
            .iter()
            .map(|bucket| bucket.id.as_str())
            .min()
            .unwrap_or("default")
    }
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
pub(crate) struct Catalog {
    pools: HashMap<String, String>,
}

impl Catalog {
    /// Membership comes from AGY's explicit family list in each group. Unknown
    /// language or ambiguous membership degrades to one default pool.
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
            .unwrap_or_else(|| {
                warn();
                "default"
            });
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
        eprintln!("warning: unrecognized AGY metadata; using no effort options and an account-scoped default quota pool");
    }
}

type Runtime = (String, Option<PathBuf>);
struct Cached {
    at: Instant,
    catalog: Catalog,
}
static CACHE: OnceLock<Mutex<HashMap<Runtime, Cached>>> = OnceLock::new();

/// Resolve capacity using this installation's live inventory, throttling both
/// successful and failed probes. Refresh replaces the inventory, removing retirees.
pub(crate) fn pool(account: &str, model: &str, executable: &str, home: Option<&Path>) -> String {
    #[cfg(test)]
    if let Some(catalog) = tests::FIXTURE.with(|cell| cell.borrow().clone()) {
        return catalog.pool(account, model);
    }
    let key = (executable.to_string(), home.map(Path::to_path_buf));
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if cache
        .get(&key)
        .is_none_or(|cached| cached.at.elapsed() >= TTL)
    {
        let catalog = load(executable, home).unwrap_or_else(|_| {
            warn();
            Catalog::default()
        });
        cache.insert(
            key.clone(),
            Cached {
                at: Instant::now(),
                catalog,
            },
        );
    }
    cache[&key].catalog.pool(account, model)
}

fn load(executable: &str, home: Option<&Path>) -> Result<Catalog> {
    let models = crate::usage::agy::run(executable, home, &["models"], 10)?;
    let usage = crate::usage::agy::read_usage(executable, home)?;
    Catalog::parse(std::str::from_utf8(&models)?, &usage)
}

#[cfg(test)]
#[path = "agy_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) use tests::fixture;
