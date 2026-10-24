use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// Selects orchestration configuration only; cloud clients are configured separately.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationMode {
    #[default]
    Local,
    Hosted,
    SelfHosted,
}

impl std::fmt::Display for OrchestrationMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Local => "local",
            Self::Hosted => "hosted",
            Self::SelfHosted => "self_hosted",
        })
    }
}

impl OrchestrationMode {
    pub fn validate(self) -> Result<()> {
        self.validate_with_env(|name| std::env::var(name).ok())
    }

    fn validate_with_env(self, env: impl Fn(&str) -> Option<String>) -> Result<()> {
        if self == Self::Local {
            return Ok(());
        }
        for name in ["GAH_SUPABASE_URL", "GAH_SUPABASE_ANON_KEY"] {
            if env(name).is_none_or(|value| value.trim().is_empty()) {
                bail!(
                    "orchestration_mode = \"{self}\" requires {name}; export {name} before starting gah, or use [defaults] orchestration_mode = \"local\""
                );
            }
        }
        // The service-role key belongs only to the server, never CLI config.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GahConfig;

    #[test]
    fn config_orchestration_modes_parse_and_round_trip() {
        let config: GahConfig = toml::from_str("").unwrap();
        assert_eq!(config.defaults.orchestration_mode, OrchestrationMode::Local);
        for (name, mode) in [
            ("local", OrchestrationMode::Local),
            ("hosted", OrchestrationMode::Hosted),
            ("self_hosted", OrchestrationMode::SelfHosted),
        ] {
            let config: GahConfig =
                toml::from_str(&format!("[defaults]\norchestration_mode = \"{name}\"")).unwrap();
            assert_eq!(config.defaults.orchestration_mode, mode);
            assert_eq!(mode.to_string(), name);
            let saved = toml::to_string(&config).unwrap();
            let loaded: GahConfig = toml::from_str(&saved).unwrap();
            assert_eq!(loaded.defaults.orchestration_mode, mode);
        }
        let error = toml::from_str::<GahConfig>("[defaults]\norchestration_mode = \"unknown\"")
            .unwrap_err()
            .to_string();
        for name in ["local", "hosted", "self_hosted"] {
            assert!(error.contains(name), "{error}");
        }
    }

    #[test]
    fn config_orchestration_credentials_are_required_only_in_cloud_modes() {
        OrchestrationMode::Local
            .validate_with_env(|_| panic!("local mode must not read cloud credentials"))
            .unwrap();
        for mode in [OrchestrationMode::Hosted, OrchestrationMode::SelfHosted] {
            for name in ["GAH_SUPABASE_URL", "GAH_SUPABASE_ANON_KEY"] {
                for missing in [None, Some(""), Some(" \t ")] {
                    let error = mode
                        .validate_with_env(|key| {
                            if key == name {
                                missing.map(String::from)
                            } else {
                                Some("obviously-fake-credential".into())
                            }
                        })
                        .unwrap_err()
                        .to_string();
                    assert!(error.contains(name), "{error}");
                    assert!(error.contains("export"), "{error}");
                    assert!(!error.contains("obviously-fake-credential"));
                }
            }
            mode.validate_with_env(|key| match key {
                "GAH_SUPABASE_URL" => Some("https://supabase.example.invalid".into()),
                "GAH_SUPABASE_ANON_KEY" => Some("obviously-fake-anon-key".into()),
                _ => panic!("CLI must not read the service-role key"),
            })
            .unwrap();
        }
    }
}
