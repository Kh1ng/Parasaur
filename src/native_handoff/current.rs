//! Recheck the central inputs that can change after context preparation.
//! This is one prerequisite for a grant, never a grant or worker admission.
use super::*;

/// Read current central configuration/history, not the prepared history, before
/// authorizing a start or another attempt. No backend or claim is created.
/// Spend holds retain the shared admission guard's deduplicated notification.
/// The caller must still check loop state, exact ownership, live work/review
/// eligibility, account approval and host reservation before issuing authority.
pub fn check_current_policy(
    cfg: &GahConfig,
    context: &Path,
    expected: &str,
    node_id: &str,
    origin: &str,
) -> Result<DispatchBinding> {
    ensure!(
        crate::node_role::NodeRoleStatus::resolve(&cfg.defaults)?.role
            == crate::node_role::NodeRole::Central,
        "Only the central node can check current native policy"
    );
    ensure!(
        cfg.defaults.factory_enabled != Some(false),
        "Factory execution is disabled centrally"
    );
    private_directory(context)?;
    let bytes = read_blob(&context.join("manifest.json"), None, MAX_JSON)?;
    ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == expected,
        "Native context manifest does not match the central offer"
    );
    let manifest: Manifest = read_json(&bytes)?;
    ensure!(
        manifest.version == 2
            && manifest.ledger_schema == crate::ledger::LEDGER_SCHEMA_VERSION
            && manifest.review_contract == crate::ledger::REVIEW_CONTRACT_VERSION
            && manifest.node_id == node_id
            && manifest.central_url == central_url(origin)?,
        "Native context identity does not match the central policy check"
    );
    let request: JobRequest = read_json(&read_blob(
        &context.join("request.json"),
        Some(
            manifest
                .files
                .get("request.json")
                .context("Prepared request is missing")?,
        ),
        MAX_JSON,
    )?)?;
    ensure!(
        request.args.profile == manifest.profile
            && request.args.run_id.as_deref() == Some(manifest.run_id.as_str()),
        "Native request identity does not match its manifest"
    );
    let prepared: policy::PortablePolicy = read_json(&read_blob(
        &context.join("policy.json"),
        Some(
            manifest
                .files
                .get("policy.json")
                .context("Prepared policy is missing")?,
        ),
        MAX_JSON,
    )?)?;
    let current = policy::prepare(cfg, &request.args.profile)?;
    ensure!(
        serde_json::to_value(&prepared)? == serde_json::to_value(&current)?,
        "Central native policy changed; prepare a new context after reconciling this run"
    );
    let profile = crate::config::get_profile(cfg, &request.args.profile)?;
    match (
        profile.policy_path.as_deref(),
        manifest.files.get("repository-policy.toml"),
    ) {
        (None, None) => {}
        (Some(path), Some(blob)) => {
            let mut bytes = Vec::new();
            File::open(path)?
                .take(MAX_REPO_POLICY + 1)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() as u64 <= MAX_REPO_POLICY && bytes.len() as u64 == blob.bytes
                && format!("{:x}",Sha256::digest(&bytes)) == blob.sha256,
                "Central repository policy changed; prepare a new context after reconciling this run");
        }
        _ => anyhow::bail!(
            "Central repository policy changed; prepare a new context after reconciling this run"
        ),
    }
    let binding = binding(&request, node_id, &current, expected)?;
    // Streaming evaluation of the live central ledger includes spend recorded
    // after the immutable context snapshot. Unknown cost remains unknown.
    crate::spend_budget::ensure_available(cfg, &request.args.profile)?;
    Ok(binding)
}
