use super::{
    default_config_path, load_canonical_routing, merge_routing_policy, resolve_config_path,
    Defaults, GahConfig, Profile,
};
use anyhow::{Context, Result};

pub fn load(config_path: Option<&str>) -> Result<GahConfig> {
    let path = resolve_config_path(config_path);
    if !path.exists() {
        anyhow::bail!(
            "no config found; set PARASAUR_CONFIG or create {}",
            default_config_path().display()
        );
    }

    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut cfg: GahConfig =
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    cfg.defaults.orchestration_mode.validate()?;
    // Invalid values must not lock the operator out of the config; the
    // rationale lives on NodeCapacitySettings::sanitized.
    cfg.defaults.node_capacity = cfg.defaults.node_capacity.sanitized();
    if let Some(canonical_routing) = load_canonical_routing()? {
        cfg.defaults.routing = merge_routing_policy(canonical_routing, cfg.defaults.routing);
    }

    // Lint candidate model consistency
    if let Some(budget) = &cfg.defaults.routing.spend_budget {
        budget.validate()?;
    }
    for (name, profile) in &cfg.profiles {
        if let Some(budget) = profile.effective_routing(&cfg.defaults).spend_budget {
            budget
                .validate()
                .with_context(|| format!("profile '{name}' spend budget"))?;
        }
        if let Err(mismatches) = check_profile_candidate_model_consistency(&cfg.defaults, profile) {
            for m in mismatches {
                eprintln!("WARNING: Profile '{}': {}", name, m);
            }
        }
    }

    Ok(cfg)
}

pub fn check_profile_candidate_model_consistency(
    defaults: &Defaults,
    profile: &Profile,
) -> Result<(), Vec<String>> {
    let routing = profile.effective_routing(defaults);
    let candidates = routing.labeled_candidates();
    let mut errors = routing.allowed_model_errors();
    for (label, candidate) in candidates {
        // An allow-list entry without a model admits any model on its backend.
        if label == "allowed_model" && candidate.model.is_none() {
            continue;
        }
        let args = match candidate.backend.as_str() {
            "codex" => &profile.codex_args,
            "opencode" => &profile.opencode_args,
            "claude" => &profile.claude_args,
            _ => continue,
        };
        if let Some(pinned) =
            crate::runner::extract_model_from_backend_args(&candidate.backend, args)
        {
            if candidate.model.as_deref() != Some(&pinned) {
                errors.push(format!(
                    "candidate {} (backend '{}', label '{}') has mismatch with profile backend args model pin '{}'",
                    label,
                    candidate.backend,
                    candidate.model.as_deref().unwrap_or("None"),
                    pinned
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
