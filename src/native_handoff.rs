//! A native job's immutable central policy/history input. Preparing or checking
//! this context does not authorize execution; the fleet still owns admission,
//! claims, authorization refresh, process lifetime and result reconciliation.
use crate::{config::GahConfig, dispatch::DispatchArgs, ledger::remote_dispatch::DispatchBinding};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::Path,
};

mod current;
mod history;
mod policy;
mod work;
pub use current::check_current_policy;
pub use work::check_current_work;

const MAX_JSON: u64 = 8 * 1024 * 1024;
const MAX_HISTORY: u64 = 128 * 1024 * 1024;
const MAX_HISTORY_CHUNKS: u32 = 8;
const MAX_REPO_POLICY: u64 = 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobRequest {
    pub args: DispatchArgs,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Blob {
    sha256: String,
    bytes: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    ledger_schema: u32,
    review_contract: u32,
    created_at: String,
    central_url: String,
    node_id: String,
    profile: String,
    run_id: String,
    history_entries: u64,
    history_chunks: u32,
    files: BTreeMap<String, Blob>,
}

pub struct WorkerContext {
    pub config: GahConfig,
    pub request: JobRequest,
    pub binding: DispatchBinding,
    pub history_entries: u64,
}

fn central_url(value: &str) -> Result<String> {
    let mut url = url::Url::parse(value)?;
    ensure!(
        matches!(url.scheme(), "https" | "http")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "A native context requires a credential-free central HTTP origin"
    );
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn private_directory(path: &Path) -> Result<()> {
    let stat = fs::symlink_metadata(path)?;
    ensure!(
        stat.is_dir() && stat.uid() == unsafe { libc::geteuid() } && stat.mode() & 0o077 == 0,
        "Native context directories must be private, owned directories"
    );
    Ok(())
}

fn create_directory(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute() && path.file_name().is_some(),
        "Use an absolute owned native job directory"
    );
    fs::create_dir_all(
        path.parent()
            .context("Native job directory has no parent")?,
    )?;
    fs::DirBuilder::new().mode(0o700).create(path).context(
        "Native job directory already exists or cannot be created; retain it for reconciliation",
    )?;
    private_directory(path)
}

fn open_blob(path: &Path, max: u64) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let stat = file.metadata()?;
    ensure!(
        stat.is_file()
            && stat.uid() == unsafe { libc::geteuid() }
            && stat.mode() & 0o077 == 0
            && stat.len() <= max,
        "Native context file is not a private bounded regular file"
    );
    Ok(file)
}

fn read_json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("Native context does not match the supported schema"))
}

fn copy_blob(path: &Path, expected: Option<&Blob>, max: u64, mut out: impl Write) -> Result<Blob> {
    let mut file = open_blob(path, max)?;
    let mut digest = Sha256::new();
    let mut count = 0;
    let mut buffer = [0u8; 16384];
    loop {
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        count += size as u64;
        ensure!(count <= max, "Native context file grew beyond its limit");
        digest.update(&buffer[..size]);
        out.write_all(&buffer[..size])?;
    }
    let actual = Blob {
        sha256: format!("{:x}", digest.finalize()),
        bytes: count,
    };
    if let Some(expected) = expected {
        ensure!(
            actual.sha256 == expected.sha256 && actual.bytes == expected.bytes,
            "Native context content does not match its manifest"
        );
    }
    Ok(actual)
}

fn read_blob(path: &Path, expected: Option<&Blob>, max: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    copy_blob(path, expected, max, &mut bytes)?;
    Ok(bytes)
}

fn write_blob(root: &Path, name: &str, bytes: &[u8]) -> Result<Blob> {
    crate::installer::files::replace(&root.join(name), bytes, 0o600)?;
    Ok(Blob {
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len() as u64,
    })
}

fn binding(
    request: &JobRequest,
    node_id: &str,
    policy: &policy::PortablePolicy,
    digest: &str,
) -> Result<DispatchBinding> {
    let args = &request.args;
    let run_id = args
        .run_id
        .as_deref()
        .context("A native job requires its central run id")?;
    let parsed = uuid::Uuid::parse_str(run_id)?;
    ensure!(
        !parsed.is_nil()
            && parsed.to_string() == run_id
            && !node_id.is_empty()
            && node_id.len() <= 128
            && node_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "Invalid native run or execution node"
    );
    let work_id = args
        .work_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .context("A native job requires its central work identity")?;
    let kind = crate::job_kind::JobKind::parse(&args.mode)?;
    let generation = args.expected_review_generation.as_ref();
    ensure!(
        kind != crate::job_kind::JobKind::Review || generation.is_some(),
        "A native review requires its selected central generation"
    );
    ensure!(
        !args.current_branch,
        "Resolve the central branch explicitly before native handoff"
    );
    let string = |key: &str| {
        policy.fields[key]
            .as_str()
            .map(String::from)
            .context("Native policy is missing repository identity")
    };
    Ok(DispatchBinding {
        run_id: run_id.into(),
        context_sha256: digest.into(),
        node_id: node_id.into(),
        profile: args.profile.clone(),
        repo_id: string("repo_id")?,
        repo: string("repo")?,
        work_id: work_id.into(),
        mode: args.mode.clone(),
        branch: args.existing_branch.clone().or_else(|| args.branch.clone()),
        review_generation: generation.cloned(),
        dispatch_reason: args.dispatch_reason.clone(),
        backend: args.backend.clone(),
        backend_instance: args.backend_instance.clone(),
        model: args.model.clone(),
    })
}

/// Freeze one central job's explicit policy and complete profile history.
/// The destination must be new; a partial or previous context is never replaced.
pub fn prepare(
    cfg: &GahConfig,
    request: &JobRequest,
    node_id: &str,
    origin: &str,
    output: &Path,
) -> Result<DispatchBinding> {
    let profile = crate::config::get_profile(cfg, &request.args.profile)?;
    let portable = policy::prepare(cfg, &request.args.profile)?;
    let mut request = request.clone();
    request.args.route_admission = None;
    let target = Path::new(&request.args.target);
    if target.is_absolute() {
        request.args.target = target
            .strip_prefix(Path::new(&profile.local_path))
            .context("An absolute native target must be inside the configured repository")?
            .to_string_lossy()
            .into_owned();
    }
    ensure!(
        !Path::new(&request.args.target)
            .components()
            .any(|component| matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )),
        "Native target must not escape the configured repository"
    );
    let preliminary = binding(&request, node_id, &portable, "")?;
    let origin = central_url(origin)?;
    let request_bytes = serde_json::to_vec(&request)?;
    let mut safe: serde_json::Value = read_json(&request_bytes)?;
    crate::redact::redact_json_value(&mut safe);
    ensure!(
        safe == read_json::<serde_json::Value>(&request_bytes)?,
        "Native request contains a secret; redact prior evidence before handoff"
    );
    let policy_bytes = serde_json::to_vec(&portable)?;
    ensure!(
        request_bytes.len() as u64 <= MAX_JSON && policy_bytes.len() as u64 <= MAX_JSON,
        "Native request or policy exceeds its bounded input limit"
    );
    let repository_policy = profile
        .policy_path
        .as_ref()
        .map(|path| -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            File::open(path)?
                .take(MAX_REPO_POLICY + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= MAX_REPO_POLICY,
                "Repository policy exceeds its context limit"
            );
            let text = std::str::from_utf8(&bytes)?;
            parse_repository_policy(&bytes)?;
            ensure!(
                crate::redact::redact(text) == text,
                "Repository policy contains a secret"
            );
            Ok(bytes)
        })
        .transpose()?;
    crate::dispatch::ensure_minimum_free_space(
        output,
        "native context filesystem",
        MAX_HISTORY * u64::from(MAX_HISTORY_CHUNKS),
    )?;
    create_directory(output)?;
    let mut files = BTreeMap::new();
    files.insert(
        "request.json".into(),
        write_blob(output, "request.json", &request_bytes)?,
    );
    files.insert(
        "policy.json".into(),
        write_blob(output, "policy.json", &policy_bytes)?,
    );
    let mut count = 0u64;
    let mut history = history::Writer::new(output, MAX_HISTORY, MAX_HISTORY_CHUNKS);
    crate::ledger::visit_entries(cfg, |entry| {
        if entry.profile != request.args.profile {
            return Ok(());
        }
        ensure!(
            (1..=crate::ledger::LEDGER_SCHEMA_VERSION).contains(&entry.schema_version),
            "Profile history contains an unsupported ledger schema"
        );
        let mut value = serde_json::to_value(&entry)?;
        crate::redact::redact_json_value(&mut value);
        let bytes = serde_json::to_vec(&value)?;
        history.append(&bytes)?;
        count += 1;
        Ok(())
    })?;
    let chunks = history.finish()?;
    let history_chunks = chunks.len() as u32;
    files.extend(chunks);
    if let Some(bytes) = repository_policy {
        files.insert(
            "repository-policy.toml".into(),
            write_blob(output, "repository-policy.toml", &bytes)?,
        );
    }
    let manifest = Manifest {
        version: 2,
        ledger_schema: crate::ledger::LEDGER_SCHEMA_VERSION,
        review_contract: crate::ledger::REVIEW_CONTRACT_VERSION,
        created_at: crate::clock::now_utc()
            .format(&time::format_description::well_known::Rfc3339)?,
        central_url: origin,
        node_id: node_id.into(),
        profile: request.args.profile.clone(),
        run_id: preliminary.run_id.clone(),
        history_entries: count,
        history_chunks,
        files,
    };
    let blob = write_blob(output, "manifest.json", &serde_json::to_vec(&manifest)?)?;
    let binding = DispatchBinding {
        context_sha256: blob.sha256,
        ..preliminary
    };
    write_blob(output, "binding.json", &serde_json::to_vec(&binding)?)?;
    File::open(output)?.sync_all()?;
    File::open(output.parent().unwrap())?.sync_all()?;
    Ok(binding)
}

/// Verify an expected context and materialize its history in a new private job
/// directory. Local credentials, executables and node capacity are preserved.
/// This is preparation only; it neither starts a backend nor refreshes authority.
pub fn load(
    worker: &GahConfig,
    context: &Path,
    expected: &str,
    node_id: &str,
    work: &Path,
) -> Result<WorkerContext> {
    for (key, file) in [
        ("GAH_LEDGER_PATH", "ledger.jsonl"),
        ("GAH_EVENTS_PATH", "events.jsonl"),
        ("GAH_RECONCILIATION_PATH", "reconciliation.jsonl"),
    ] {
        ensure!(
            std::env::var_os(key).is_none_or(|value| Path::new(&value) == work.join(file)),
            "An inherited state override would bypass the native job context"
        );
    }
    private_directory(context)?;
    let manifest_bytes = read_blob(&context.join("manifest.json"), None, MAX_JSON)?;
    ensure!(
        format!("{:x}", Sha256::digest(&manifest_bytes)) == expected,
        "Native context manifest does not match the central offer"
    );
    let manifest: Manifest = read_json(&manifest_bytes)?;
    ensure!(
        manifest.version == 2
            && manifest.ledger_schema == crate::ledger::LEDGER_SCHEMA_VERSION
            && manifest.review_contract == crate::ledger::REVIEW_CONTRACT_VERSION
            && manifest.node_id == node_id,
        "Native context version or execution node does not match this worker"
    );
    let configured = worker
        .defaults
        .registry_central_url
        .as_deref()
        .context("Worker requires its configured central authority")?;
    ensure!(
        central_url(configured)? == manifest.central_url,
        "Native context does not belong to this worker's configured central authority"
    );
    ensure!(
        worker.defaults.factory_enabled != Some(false),
        "Factory execution is disabled on this worker"
    );
    ensure!(
        (1..=MAX_HISTORY_CHUNKS).contains(&manifest.history_chunks),
        "Native context has an unsupported history chunk count"
    );
    let history_names: Vec<String> = (0..manifest.history_chunks).map(history::name).collect();
    for name in ["request.json", "policy.json"]
        .into_iter()
        .chain(history_names.iter().map(String::as_str))
    {
        ensure!(
            manifest.files.contains_key(name),
            "Native context is incomplete"
        );
    }
    ensure!(
        manifest
            .files
            .keys()
            .all(|name| history_names.contains(name)
                || matches!(
                    name.as_str(),
                    "request.json" | "policy.json" | "repository-policy.toml"
                )),
        "Native context names an unsupported input"
    );
    let request: JobRequest = read_json(&read_blob(
        &context.join("request.json"),
        manifest.files.get("request.json"),
        MAX_JSON,
    )?)?;
    let policy: policy::PortablePolicy = read_json(&read_blob(
        &context.join("policy.json"),
        manifest.files.get("policy.json"),
        MAX_JSON,
    )?)?;
    ensure!(
        request.args.profile == manifest.profile
            && request.args.run_id.as_deref() == Some(manifest.run_id.as_str()),
        "Native request identity does not match its manifest"
    );
    let binding = binding(&request, node_id, &policy, expected)?;
    let mut cfg = policy::apply(&policy, worker, &request.args)?;
    let central_policy = manifest
        .files
        .get("repository-policy.toml")
        .map(|blob| {
            read_blob(
                &context.join("repository-policy.toml"),
                Some(blob),
                MAX_REPO_POLICY,
            )
        })
        .transpose()?;
    let local_policy = crate::config::get_profile(worker, &request.args.profile)?
        .policy_path
        .as_ref()
        .map(|path| -> Result<Vec<u8>> {
            let mut bytes = Vec::new();
            File::open(path)?
                .take(MAX_REPO_POLICY + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 <= MAX_REPO_POLICY,
                "Worker repository policy exceeds its input limit"
            );
            Ok(bytes)
        })
        .transpose()?;
    let combined_policy = intersect_policy(central_policy.as_deref(), local_policy.as_deref())?;
    let history_bytes = history_names
        .iter()
        .try_fold(0u64, |total, name| -> Result<u64> {
            let bytes = manifest.files[name].bytes;
            ensure!(
                bytes <= MAX_HISTORY,
                "Native history chunk exceeds its bounded input limit"
            );
            Ok(total + bytes)
        })?;
    crate::dispatch::ensure_minimum_free_space(work, "native job filesystem", history_bytes)?;
    create_directory(work)?;
    let history_path = work.join("ledger.jsonl");
    crate::installer::files::replace_with(&history_path, 0o600, |file| {
        for name in &history_names {
            copy_blob(
                &context.join(name),
                manifest.files.get(name),
                MAX_HISTORY,
                &mut *file,
            )?;
        }
        Ok(())
    })?;
    let mut count = 0;
    for line in BufReader::new(File::open(&history_path)?).lines() {
        let entry: crate::ledger::LedgerEntry = read_json(line?.as_bytes())?;
        ensure!(
            entry.profile == request.args.profile
                && (1..=crate::ledger::LEDGER_SCHEMA_VERSION).contains(&entry.schema_version),
            "Native history has an unsupported entry or profile scope"
        );
        count += 1;
    }
    ensure!(
        count == manifest.history_entries,
        "Native context history is incomplete"
    );
    cfg.defaults.artifact_root = work.to_string_lossy().into_owned();
    let profile = cfg.profiles.get_mut(&request.args.profile).unwrap();
    profile.artifact_root = work.to_string_lossy().into_owned();
    if let Some(bytes) = combined_policy {
        write_blob(work, "repository-policy.toml", &bytes)?;
        profile.policy_path = Some(
            work.join("repository-policy.toml")
                .to_string_lossy()
                .into_owned(),
        );
    }
    File::open(work)?.sync_all()?;
    File::open(work.parent().unwrap())?.sync_all()?;
    Ok(WorkerContext {
        config: cfg,
        request,
        binding,
        history_entries: count,
    })
}

fn intersect_policy(central: Option<&[u8]>, local: Option<&[u8]>) -> Result<Option<Vec<u8>>> {
    let result = match (central, local) {
        (None, None) => return Ok(None),
        (Some(bytes), None) | (None, Some(bytes)) => parse_repository_policy(bytes)?,
        (Some(central), Some(local)) => {
            let a = parse_repository_policy(central)?.repo;
            let b = parse_repository_policy(local)?.repo;
            crate::models::PolicyConfig {
                repo: crate::models::RepoPolicy {
                    trust_mode: if a.trust_mode == "draft_pr_allowed"
                        && b.trust_mode == "draft_pr_allowed"
                    {
                        "draft_pr_allowed"
                    } else {
                        "read_only"
                    }
                    .into(),
                    allow_provider_mutation: a.allow_provider_mutation && b.allow_provider_mutation,
                    allow_push: a.allow_push && b.allow_push,
                    allow_draft_pr: a.allow_draft_pr && b.allow_draft_pr,
                    allow_issue_write: a.allow_issue_write && b.allow_issue_write,
                    allow_project_write: a.allow_project_write && b.allow_project_write,
                },
            }
        }
    };
    Ok(Some(toml::to_string(&result)?.into_bytes()))
}

fn parse_repository_policy(bytes: &[u8]) -> Result<crate::models::PolicyConfig> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| toml::from_str(text).ok())
        .context("Repository policy does not match the supported schema")
}

#[cfg(test)]
mod tests;
