//! Central controller handoff. A native reservation must be acknowledged before
//! dropping the controller's local claim; activation is a separate operation.
use crate::{config::GahConfig, dispatch::DispatchArgs};
#[cfg(unix)]
use anyhow::ensure;
#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::Result;
#[cfg(target_os = "linux")]
use serde::Deserialize;

pub(super) fn enabled(cfg: &GahConfig) -> Result<bool> {
    Ok(
        crate::node_role::NodeRoleStatus::resolve(&cfg.defaults)?.role
            == crate::node_role::NodeRole::Central
            && cfg.defaults.registry_central_url.is_some(),
    )
}

#[derive(Deserialize)]
#[cfg(target_os = "linux")]
struct RunView {
    binding: crate::ledger::remote_dispatch::DispatchBinding,
    phase: String,
    activated: Option<bool>,
}
pub(super) struct Offered {
    pub run_id: String,
    pub detail: String,
}

#[cfg(target_os = "linux")]
fn request(cfg: &GahConfig, path: &str, body: &serde_json::Value) -> Result<Vec<u8>> {
    let origin = cfg
        .defaults
        .registry_central_url
        .as_deref()
        .context("Native central URL unavailable")?;
    let mut url = url::Url::parse(origin)?;
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "Invalid native central origin"
    );
    url.set_path(&format!("/api/native-dispatch/{path}"));
    let token = std::env::var("COORDINATOR_TOKEN")
        .context("Central native service credential unavailable")?;
    let response =
        crate::curl_http::request_controller(url.as_str(), &serde_json::to_string(body)?, &token)?;
    ensure!(
        response.status == 200,
        "Central native {path} unconfirmed (HTTP {}{})",
        response.status,
        refusal_reason(&response.body)
    );
    Ok(response.body)
}

/// The central's stated reason for refusing, so the loop's log says what to fix.
#[cfg(target_os = "linux")]
fn refusal_reason(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|reply| Some(format!(": {}", reply.get("reason")?.as_str()?)))
        .unwrap_or_default()
}

#[cfg(target_os = "linux")]
pub(super) fn submit(cfg: &GahConfig, args: &DispatchArgs) -> Result<Offered> {
    let bytes = request(cfg, "reserve", &serde_json::json!({"args":args}))?;
    let view: RunView = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid central native reservation"))?;
    let work = args
        .work_id
        .as_deref()
        .context("Native controller requires work identity")?;
    let binding = &view.binding;
    ensure!(
        binding.profile == args.profile
            && crate::work_claim::normalize_work_identity(&binding.work_id)
                == crate::work_claim::normalize_work_identity(work)
            && uuid::Uuid::parse_str(&binding.run_id)
                .is_ok_and(|id| !id.is_nil() && id.to_string() == binding.run_id)
            && !binding.node_id.is_empty()
            && binding.node_id.len() <= 128
            && binding.context_sha256.len() == 64
            && binding
                .context_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            && [
                "prepared",
                "reserved",
                "stage_offered",
                "waiting_capacity",
                "authorization_deferred",
                "start_offered",
                "running",
                "reconciling",
                "result_pending",
                "terminal",
                "cancelling",
                "cancelled"
            ]
            .contains(&view.phase.as_str()),
        "Central native reservation does not match this work"
    );
    let mut phase = view.phase.clone();
    if view.activated == Some(false) && phase == "reserved" {
        let profile = crate::config::get_profile(cfg, &args.profile)?;
        let scope = crate::work_claim::canonical_claim_scope(&args.profile, &profile.repo_id);
        crate::work_claim::release_owned_work(&scope, work)?;
        // Losing this reply leaves the same durable run active or dormant.
        // Neither outcome authorizes a local fallback or a new native start.
        phase = match request(
            cfg,
            "offer",
            &serde_json::json!({"run_id":binding.run_id,"context_sha256":binding.context_sha256}),
        ) {
            Ok(bytes) => serde_json::from_slice::<RunView>(&bytes)
                .ok()
                .filter(|reply| reply.binding == view.binding)
                .map(|reply| reply.phase)
                .unwrap_or_else(|| "activation_unconfirmed".into()),
            Err(_) => "activation_unconfirmed".into(),
        };
    }
    Ok(Offered {
        run_id: binding.run_id.clone(),
        detail: format!(
            "native run {} on worker {}: {phase}",
            binding.run_id, binding.node_id
        ),
    })
}

#[cfg(not(target_os = "linux"))]
pub(super) fn submit(_cfg: &GahConfig, _args: &DispatchArgs) -> Result<Offered> {
    anyhow::bail!("Central native controller execution requires Linux/WSL")
}

pub(super) fn apply_holds(
    cfg: &GahConfig,
    profile: &str,
    snapshot: &mut crate::status::StatusSnapshot,
) -> Result<()> {
    if !enabled(cfg)? {
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let bytes = request(cfg, "holds", &serde_json::json!({"profile":profile}))?;
        let holds: Vec<crate::central_claims::NativeHeldWork> = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Native ownership observation invalid"))?;
        project_native_holds(profile, None, &holds, None)?;
        if cfg.defaults.factory_enabled != Some(false) {
            resume_dormant(cfg, profile, &holds)?;
        }
        project_native_holds(profile, Some(snapshot), &holds, None)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (profile, snapshot);
        anyhow::bail!("Central native scheduling requires Linux/WSL")
    }
}

#[cfg(target_os = "linux")]
fn resume_dormant(
    cfg: &GahConfig,
    profile: &str,
    holds: &[crate::central_claims::NativeHeldWork],
) -> Result<()> {
    if holds.is_empty() {
        return Ok(());
    }
    let bytes = request(
        cfg,
        "pending-activation",
        &serde_json::json!({"profile":profile}),
    )?;
    let pending: Vec<RunView> = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Native dormant reservation observation invalid"))?;
    let configured = crate::config::get_profile(cfg, profile)?;
    let mut ids = std::collections::HashSet::new();
    ensure!(
        pending.len() <= holds.len()
            && pending.iter().all(|view| {
                let binding = &view.binding;
                view.activated == Some(false)
                    && view.phase == "reserved"
                    && binding.profile == profile
                    && binding.repo_id == configured.repo_id
                    && binding.repo == configured.repo
                    && ids.insert(binding.run_id.clone())
                    && holds.iter().any(|hold| {
                        hold.run_id == binding.run_id
                            && hold.node_id == binding.node_id
                            && hold.context_sha256 == binding.context_sha256
                            && crate::work_claim::normalize_work_identity(&hold.work_id)
                                == crate::work_claim::normalize_work_identity(&binding.work_id)
                    })
            }),
        "Dormant reservation does not match canonical ownership"
    );
    let scope = crate::work_claim::canonical_claim_scope(profile, &configured.repo_id);
    for view in pending {
        // A live local owner may still be receiving its reservation reply.
        // Never release another dispatch's claim to make recovery progress.
        if crate::work_claim::get_claimed_work_ids(&scope)?
            .iter()
            .any(|work| *work == crate::work_claim::normalize_work_identity(&view.binding.work_id))
        {
            continue;
        }
        // The API rechecks canonical ownership and fresh work/route authority.
        // An uncertain reply remains the same run; the next cycle observes it.
        let _ = request(
            cfg,
            "offer",
            &serde_json::json!({
                "run_id":view.binding.run_id,"context_sha256":view.binding.context_sha256
            }),
        );
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn project_native_holds(
    profile: &str,
    snapshot: Option<&mut crate::status::StatusSnapshot>,
    holds: &[crate::central_claims::NativeHeldWork],
    exclude_run: Option<&str>,
) -> Result<()> {
    let mut ids = std::collections::HashSet::new();
    let mut work = std::collections::HashSet::new();
    ensure!(
        holds.len() <= 4096
            && holds.iter().all(|hold| hold.profile == profile
                && !hold.work_id.is_empty()
                && hold.work_id.len() <= 4096
                && !hold.work_id.chars().any(char::is_control)
                && !hold.node_id.is_empty()
                && hold.node_id.len() <= 128
                && hold.context_sha256.len() == 64
                && hold
                    .context_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                && uuid::Uuid::parse_str(&hold.run_id)
                    .is_ok_and(|id| !id.is_nil() && id.to_string() == hold.run_id)
                && ids.insert(hold.run_id.clone())
                && work.insert(crate::work_claim::normalize_work_identity(&hold.work_id))),
        "Native ownership observation exceeds its scope"
    );
    let Some(snapshot) = snapshot else {
        return Ok(());
    };
    let projected = holds
        .iter()
        .filter(|hold| Some(hold.run_id.as_str()) != exclude_run)
        .collect::<Vec<_>>();
    let local = snapshot
        .active_claims
        .iter()
        .map(|claim| crate::work_claim::normalize_work_identity(&claim.work_id))
        .collect::<std::collections::HashSet<_>>();
    let added = projected
        .iter()
        .filter(|hold| {
            hold.reserves_intake
                && !local.contains(&crate::work_claim::normalize_work_identity(&hold.work_id))
        })
        .count() as u32;
    snapshot.inflight_implementation_count =
        snapshot.inflight_implementation_count.saturating_add(added);
    snapshot.implementation_intake_paused = snapshot
        .open_managed_mr_count
        .saturating_add(snapshot.inflight_implementation_count)
        >= snapshot.profile.max_open_managed_mrs;
    let work = projected
        .iter()
        .map(|hold| crate::work_claim::normalize_work_identity(&hold.work_id))
        .collect::<Vec<_>>();
    super::retain_unclaimed_work(snapshot, &work, &std::collections::HashSet::new());
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "native_dispatch/tests.rs"]
mod tests;
