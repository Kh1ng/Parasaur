//! Vibe's config-driven model selection (#1510).
//!
//! Vibe's CLI has no `--model` flag: the offered models and the active one
//! live in its own config (`models` and `active_model` in
//! `<vibe home>/config.toml`). GAH's picker reads that file through this
//! module and persists a choice back into it — no network call, so the
//! list reflects exactly what the operator's Vibe is configured to run.

use anyhow::{anyhow, bail, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

/// One selectable model as the picker shows it: `id` is the value Vibe's
/// `active_model` understands (the entry's alias, or its name when no
/// alias is set); `name` is the display label.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct VibeModelOption {
    pub(crate) id: String,
    pub(crate) name: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct VibeModelSelection {
    pub(crate) models: Vec<VibeModelOption>,
    pub(crate) active_model: Option<String>,
}

/// The Vibe home a spawned Vibe child uses: `VIBE_HOME`, else `HOME/.vibe`
/// — the same precedence the runner's session-directory resolution applies.
pub(crate) fn ambient_home() -> Option<PathBuf> {
    std::env::var_os("VIBE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".vibe")))
}

/// Read the configured models and the active one. A missing config file is
/// an unset selection, not an error: Vibe simply is not configured here.
pub(crate) fn read_selection(vibe_home: &Path) -> Result<VibeModelSelection> {
    let text = match std::fs::read_to_string(vibe_home.join("config.toml")) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(VibeModelSelection {
                models: Vec::new(),
                active_model: None,
            });
        }
        Err(_) => bail!("cannot read Vibe's configuration"),
    };
    parse_selection(&text)
}

/// Parse the picker's view of a Vibe config. `models` is written either as
/// an array of tables or as a table keyed by alias; `active_model` selects
/// by alias or name — the same resolution session metadata gets in
/// usage/vibe.rs. An active model no entry covers still appears as a
/// selectable entry so the picker can mark what Vibe will actually run.
pub(crate) fn parse_selection(config_text: &str) -> Result<VibeModelSelection> {
    let (models, active_model) = parse_configured(config_text)?;
    let mut options = models
        .iter()
        .map(|model| VibeModelOption {
            id: model.alias.clone().unwrap_or_else(|| model.name.clone()),
            name: model.display.clone().unwrap_or_else(|| model.name.clone()),
        })
        .collect::<Vec<_>>();
    if let Some(active) = &active_model {
        let covered = models
            .iter()
            .any(|model| model.alias.as_deref() == Some(active.as_str()) || model.name == *active);
        if !covered {
            options.push(VibeModelOption {
                id: active.clone(),
                name: active.clone(),
            });
        }
    }
    Ok(VibeModelSelection {
        models: options,
        active_model,
    })
}

/// Persist `model` as Vibe's `active_model` for this home, atomically
/// (temp file + rename) and byte-stable outside the edited key. Fails
/// closed when the model is neither a configured entry nor the current
/// active one, so GAH never writes a selection Vibe does not offer.
pub(crate) fn set_active_model(vibe_home: &Path, model: &str) -> Result<()> {
    let path = vibe_home.join("config.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|_| anyhow!("cannot read Vibe's configuration at {}", path.display()))?;
    let (models, active_model) = parse_configured(&text)?;
    let known = models
        .iter()
        .any(|entry| entry.alias.as_deref() == Some(model) || entry.name == model)
        || active_model.as_deref() == Some(model);
    if !known {
        bail!("model '{model}' is not configured for Vibe");
    }
    let updated = edit_root_active_model(&text, model)?;
    // Self-verifying write: any edit the round trip cannot reproduce is
    // refused, so a scanning bug can never corrupt the config.
    let verified = parse_configured(&updated)
        .map(|(_, active)| active.as_deref() == Some(model))
        .unwrap_or(false);
    if !verified {
        bail!("cannot update Vibe's active model without changing the rest of its config");
    }
    replace_file(&path, updated.as_bytes())
}

/// One `models` entry as Vibe's config defines it.
struct ConfiguredModel {
    alias: Option<String>,
    name: String,
    display: Option<String>,
}

fn parse_configured(config_text: &str) -> Result<(Vec<ConfiguredModel>, Option<String>)> {
    let config: toml::Value =
        toml::from_str(config_text).map_err(|_| anyhow!("cannot parse Vibe's configuration"))?;
    let active_model = text_field(&config, "active_model");
    let mut models = Vec::new();
    match config.get("models") {
        Some(toml::Value::Array(entries)) => {
            models.extend(entries.iter().filter_map(configured_model))
        }
        Some(toml::Value::Table(entries)) => {
            // Vibe keys its table shape by alias; an entry without its own
            // alias field still names the model by its key.
            models.extend(entries.iter().filter_map(|(key, value)| {
                configured_model(value).map(|mut model| {
                    if model.alias.is_none() {
                        model.alias = Some(key.clone());
                    }
                    model
                })
            }))
        }
        _ => {}
    }
    Ok((models, active_model))
}

fn text_field(value: &toml::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn configured_model(value: &toml::Value) -> Option<ConfiguredModel> {
    value.as_table()?;
    let alias = text_field(value, "alias");
    let name = text_field(value, "name").or_else(|| alias.clone())?;
    Some(ConfiguredModel {
        alias,
        name,
        display: text_field(value, "display_name"),
    })
}

/// Replace the root-level `active_model` key, or prepend one when absent —
/// position zero is always inside the root region, so the key can never
/// land inside a table. Every other byte of the config is preserved. The
/// root scan stops at the first table header and skips multi-line string
/// bodies; the caller re-parses the result before writing.
fn edit_root_active_model(config_text: &str, model: &str) -> Result<String> {
    let replacement = format!("active_model = {}", toml_basic_string(model));
    let mut lines: Vec<String> = config_text.lines().map(str::to_string).collect();
    let mut multiline: Option<&str> = None;
    let mut replaced = false;
    for line in lines.iter_mut() {
        if let Some(delimiter) = multiline {
            if line.matches(delimiter).count() % 2 == 1 {
                multiline = None;
            }
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            break;
        }
        if let Some(rest) = trimmed.strip_prefix("active_model") {
            if rest.trim_start().starts_with('=') {
                *line = replacement.clone();
                replaced = true;
                break;
            }
        }
        for delimiter in ["\"\"\"", "'''"] {
            if line.matches(delimiter).count() % 2 == 1 {
                multiline = Some(delimiter);
            }
        }
    }
    let edited = if replaced {
        // `lines()` drops the final newline; restore exactly what the
        // original had so only the edited line differs.
        let mut joined = lines.join("\n");
        if config_text.ends_with('\n') {
            joined.push('\n');
        }
        joined
    } else {
        format!("{replacement}\n{config_text}")
    };
    Ok(edited)
}

/// Encode a TOML basic string, so an exotic alias can never break the key
/// line the edit builds.
fn toml_basic_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\u{8}' => encoded.push_str("\\b"),
            '\t' => encoded.push_str("\\t"),
            '\n' => encoded.push_str("\\n"),
            '\u{c}' => encoded.push_str("\\f"),
            '\r' => encoded.push_str("\\r"),
            control if control < '\u{20}' => {
                encoded.push_str(&format!("\\u{:04X}", control as u32))
            }
            other => encoded.push(other),
        }
    }
    encoded.push('"');
    encoded
}

fn replace_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Vibe's config path has no parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| anyhow!("cannot update Vibe's configuration"))?;
    temp.write_all(bytes)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|_| anyhow!("cannot update Vibe's configuration"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(mode) = std::fs::metadata(path).map(|metadata| metadata.permissions().mode()) {
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(mode))
                .map_err(|_| anyhow!("cannot update Vibe's configuration"))?;
        }
    }
    temp.persist(path)
        .map_err(|_| anyhow!("cannot update Vibe's configuration"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const ARRAY_CONFIG: &str = r#"theme = "auto"
active_model = "mistral-medium-3.5"

[[models]]
name = "mistral-vibe-cli-latest"
alias = "mistral-medium-3.5"
display_name = "Mistral Medium 3.5"

[[models]]
name = "devstral"
alias = "local"
"#;

    fn home_with(config: &str) -> (tempfile::TempDir, PathBuf) {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().to_path_buf();
        fs::write(path.join("config.toml"), config).unwrap();
        (home, path)
    }

    #[test]
    fn parses_array_shaped_models_and_marks_the_active_alias() {
        let selection = parse_selection(ARRAY_CONFIG).unwrap();
        assert_eq!(
            selection.active_model.as_deref(),
            Some("mistral-medium-3.5")
        );
        assert_eq!(
            selection.models,
            vec![
                VibeModelOption {
                    id: "mistral-medium-3.5".into(),
                    name: "Mistral Medium 3.5".into()
                },
                VibeModelOption {
                    id: "local".into(),
                    name: "devstral".into()
                },
            ]
        );
    }

    #[test]
    fn parses_table_shaped_models_keyed_by_alias() {
        let selection = parse_selection(
            r#"active_model = "local"
[models."mistral-medium-3.5"]
name = "mistral-vibe-cli-latest"
display_name = "Mistral Medium 3.5"
[models.local]
name = "devstral"
"#,
        )
        .unwrap();
        assert_eq!(selection.active_model.as_deref(), Some("local"));
        assert_eq!(selection.models.len(), 2);
        // The table shape iterates keys alphabetically, so assert by lookup
        // rather than position.
        let by_id = |id: &str| {
            selection
                .models
                .iter()
                .find(|model| model.id == id)
                .unwrap_or_else(|| panic!("model '{id}' missing"))
        };
        assert_eq!(by_id("mistral-medium-3.5").name, "Mistral Medium 3.5");
        assert_eq!(by_id("local").name, "devstral");
    }

    #[test]
    fn an_active_model_no_entry_covers_stays_selectable() {
        let selection = parse_selection("theme = \"auto\"\nactive_model = \"glm-5.3\"\n").unwrap();
        assert_eq!(
            selection.models,
            vec![VibeModelOption {
                id: "glm-5.3".into(),
                name: "glm-5.3".into()
            }]
        );
        assert_eq!(selection.active_model.as_deref(), Some("glm-5.3"));
    }

    #[test]
    fn a_config_without_models_or_active_is_an_empty_selection() {
        let selection = parse_selection("theme = \"auto\"\n").unwrap();
        assert_eq!(selection.models, Vec::new());
        assert_eq!(selection.active_model, None);
    }

    #[test]
    fn an_unparsable_config_is_refused() {
        assert!(parse_selection("active_model = ").is_err());
    }

    #[test]
    fn a_missing_config_file_reads_as_an_unset_selection() {
        let home = tempfile::tempdir().unwrap();
        let selection = read_selection(home.path()).unwrap();
        assert_eq!(
            selection,
            VibeModelSelection {
                models: Vec::new(),
                active_model: None
            }
        );
    }

    #[test]
    fn set_active_model_round_trips_through_the_config() {
        let (_guard, home) = home_with(ARRAY_CONFIG);
        set_active_model(&home, "local").unwrap();
        let text = fs::read_to_string(home.join("config.toml")).unwrap();
        assert_eq!(
            text,
            r#"theme = "auto"
active_model = "local"

[[models]]
name = "mistral-vibe-cli-latest"
alias = "mistral-medium-3.5"
display_name = "Mistral Medium 3.5"

[[models]]
name = "devstral"
alias = "local"
"#,
            "only the active_model line changes"
        );
        let selection = read_selection(&home).unwrap();
        assert_eq!(selection.active_model.as_deref(), Some("local"));
    }

    #[test]
    fn set_active_model_prepends_when_no_active_key_exists() {
        let (_guard, home) = home_with(
            r#"theme = "auto"
[[models]]
name = "devstral"
alias = "local"
"#,
        );
        set_active_model(&home, "local").unwrap();
        let text = fs::read_to_string(home.join("config.toml")).unwrap();
        assert_eq!(
            text,
            "active_model = \"local\"\ntheme = \"auto\"\n[[models]]\nname = \"devstral\"\nalias = \"local\"\n",
            "the new root key lands before the first table header"
        );
        assert_eq!(
            read_selection(&home).unwrap().active_model.as_deref(),
            Some("local")
        );
    }

    #[test]
    fn set_active_model_replaces_exactly_one_line_and_leaves_no_temp_files() {
        let (_guard, home) = home_with(ARRAY_CONFIG);
        set_active_model(&home, "local").unwrap();
        let text = fs::read_to_string(home.join("config.toml")).unwrap();
        assert_eq!(text.matches("active_model").count(), 1);
        let entries: Vec<_> = fs::read_dir(&home).unwrap().collect();
        assert_eq!(entries.len(), 1, "the atomic write leaves only the config");
        assert_eq!(entries[0].as_ref().unwrap().file_name(), "config.toml");
    }

    #[test]
    fn set_active_model_skips_multiline_string_bodies_when_scanning() {
        let (_guard, home) = home_with(
            "greeting = \"\"\"\n[not a table header]\n\"\"\"\nactive_model = \"local\"\n[[models]]\nname = \"devstral\"\nalias = \"local\"\n",
        );
        set_active_model(&home, "local").unwrap();
        let text = fs::read_to_string(home.join("config.toml")).unwrap();
        assert_eq!(
            text,
            "greeting = \"\"\"\n[not a table header]\n\"\"\"\nactive_model = \"local\"\n[[models]]\nname = \"devstral\"\nalias = \"local\"\n",
            "the existing key inside the root region is replaced in place"
        );
    }

    #[test]
    fn set_active_model_rejects_unknown_models_without_touching_the_file() {
        let (_guard, home) = home_with(ARRAY_CONFIG);
        let before = fs::read_to_string(home.join("config.toml")).unwrap();
        let error = set_active_model(&home, "not-configured")
            .unwrap_err()
            .to_string();
        assert!(error.contains("not configured"), "got: {error}");
        assert_eq!(
            fs::read_to_string(home.join("config.toml")).unwrap(),
            before
        );
    }

    #[test]
    fn set_active_model_accepts_a_name_selection_and_the_current_active() {
        let (_guard, home) = home_with(
            "active_model = \"mistral-medium-3.5\"\n[[models]]\nname = \"mistral-vibe-cli-latest\"\nalias = \"mistral-medium-3.5\"\n",
        );
        set_active_model(&home, "mistral-vibe-cli-latest").unwrap();
        assert_eq!(
            read_selection(&home).unwrap().active_model.as_deref(),
            Some("mistral-vibe-cli-latest"),
            "active_model selects by alias or name, like session metadata resolution"
        );
        let (_kept, active_only) = home_with("active_model = \"glm-5.3\"\n");
        set_active_model(&active_only, "glm-5.3").unwrap();
        let (_empty, unconfigured) = home_with("theme = \"auto\"\n");
        assert!(
            set_active_model(&unconfigured, "anything").is_err(),
            "nothing is configured to be selectable"
        );
    }

    #[test]
    fn set_active_model_requires_an_existing_config() {
        let home = tempfile::tempdir().unwrap();
        let error = set_active_model(home.path(), "local")
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot read"), "got: {error}");
    }
}
