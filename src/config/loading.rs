use super::{
    check_profile_candidate_model_consistency, default_config_path, load_canonical_routing,
    merge_routing_policy, resolve_config_path, GahConfig,
};
use anyhow::{Context, Result};

pub fn load(config_path: Option<&str>) -> Result<GahConfig> {
    let path = resolve_config_path(config_path);
    if !path.exists() {
        anyhow::bail!(
            "no config found; set GAH_CONFIG or create {}",
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
    for (name, profile) in &cfg.profiles {
        if let Err(mismatches) = check_profile_candidate_model_consistency(&cfg.defaults, profile) {
            for m in mismatches {
                eprintln!("WARNING: Profile '{}': {}", name, m);
            }
        }
    }

    Ok(cfg)
}
