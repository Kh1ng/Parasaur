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
/// usage/vibe.rs. The selection is reported through the matching entry's
/// list id (its alias, or its name when no alias is set), so a picker
/// comparing ids exactly marks the active entry: with
/// `active_model = "devstral"` and an aliased `"local"` entry, the active
/// id is `"local"`, not `"devstral"`. An active model no entry covers
/// still appears as a selectable entry so the picker can mark what Vibe
/// will actually run.
pub(crate) fn parse_selection(config_text: &str) -> Result<VibeModelSelection> {
    let (models, active_model) = parse_configured(config_text)?;
    let mut options = models
        .iter()
        .map(|model| VibeModelOption {
            id: model.alias.clone().unwrap_or_else(|| model.name.clone()),
            name: model.display.clone().unwrap_or_else(|| model.name.clone()),
        })
        .collect::<Vec<_>>();
    let mut active_id = None;
    if let Some(active) = &active_model {
        match resolve_active(&models, active) {
            Some(model) => {
                active_id = Some(model.alias.clone().unwrap_or_else(|| model.name.clone()));
            }
            None => {
                options.push(VibeModelOption {
                    id: active.clone(),
                    name: active.clone(),
                });
                active_id = Some(active.clone());
            }
        }
    }
    Ok(VibeModelSelection {
        models: options,
        active_model: active_id,
    })
}

/// The configured entry `active_model` names, by alias or name — the same
/// resolution Vibe's own session metadata gets in usage/vibe.rs.
fn resolve_active<'a>(models: &'a [ConfiguredModel], active: &str) -> Option<&'a ConfiguredModel> {
    models
        .iter()
        .find(|model| model.alias.as_deref() == Some(active) || model.name == active)
}

/// Persist `model` as Vibe's `active_model` for this home, atomically
/// (temp file + rename) and byte-stable outside the edited key. The edit
/// is line-surgical — the replaced key's line ending and every other byte
/// survive — and self-verifying: the result must re-parse with `model`
/// active and every other parsed value unchanged, so a scanning bug can
/// never corrupt the config; the worst it can do is refuse. Fails closed
/// when the model is neither a configured entry nor the current active
/// one, and refuses configs outside the safe subset for a line edit
/// (multiline strings), so GAH never writes a selection Vibe does not
/// offer and never mangles a config it cannot prove it edited correctly.
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
    let edited = edit_active_model(&text, &path, model)?;
    if !edit_preserves_config(&text, &edited, model) {
        bail!("cannot update Vibe's active model without changing the rest of its config");
    }
    replace_file(&path, edited.as_bytes())
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
/// land inside a table. The replacement is spliced into the key line's
/// exact byte range, so the line's own ending (LF or CRLF) and every other
/// byte survive; a prepended key re-uses the file's detected line ending.
/// Configs containing multiline strings outside comments are refused with
/// an instruction to edit by hand: their bodies could hold a line that
/// looks exactly like a root key, and the pre-verification parse cannot
/// prove the scanner skipped them.
fn edit_active_model(config_text: &str, path: &Path, model: &str) -> Result<String> {
    let key = scan_root_active_model(config_text, path)?;
    let replacement = format!("active_model = {}", toml_basic_string(model));
    Ok(match key {
        Some((start, end)) => {
            let mut edited = String::with_capacity(config_text.len());
            edited.push_str(&config_text[..start]);
            edited.push_str(&replacement);
            edited.push_str(&config_text[end..]);
            edited
        }
        None => {
            let newline = if config_text.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            format!("{replacement}{newline}{config_text}")
        }
    })
}

/// Locate the root-level `active_model` key's byte range — the whole key
/// line's content, its line ending excluded — and prove the config is in
/// the safe subset for a line edit: no multiline strings (`"""` or `'''`)
/// anywhere outside comments. The scan tracks exactly what a naive line
/// walk cannot: `#` starts a comment only outside strings (and runs to
/// end of line), quote characters open and close strings (`\` escapes
/// the next character inside a basic string), and a `[` at line start
/// ends the root table region. `parse_configured` has already validated
/// the TOML, which is what lets this scan assume single-line strings
/// close on their own line.
fn scan_root_active_model(config_text: &str, path: &Path) -> Result<Option<(usize, usize)>> {
    let mut found = None;
    let mut root_region = true;
    let mut offset = 0;
    while offset < config_text.len() {
        let line_feed = config_text[offset..].find('\n').map(|index| offset + index);
        let line_end = line_feed.unwrap_or(config_text.len());
        let mut content_end = line_end;
        if content_end > offset && config_text.as_bytes()[content_end - 1] == b'\r' {
            content_end -= 1;
        }
        let line = &config_text[offset..content_end];
        let trimmed = line.trim_start();
        if root_region && trimmed.starts_with('[') {
            root_region = false;
        }
        if root_region && found.is_none() && trimmed.starts_with("active_model") {
            let rest = &trimmed["active_model".len()..];
            if rest.trim_start().starts_with('=') {
                found = Some((offset, content_end));
            }
        }
        let mut in_basic = false;
        let mut in_literal = false;
        let mut characters = line.char_indices().peekable();
        while let Some((index, character)) = characters.next() {
            if in_basic {
                if character == '\\' {
                    // The escaped character (including \" and \\) is
                    // string content, never a delimiter: consume it.
                    characters.next();
                } else if character == '"' {
                    in_basic = false;
                }
            } else if in_literal {
                if character == '\'' {
                    in_literal = false;
                }
            } else {
                match character {
                    '#' => break, // comment: the rest of the line is not code
                    '"' | '\'' => {
                        let opener = if character == '"' { "\"\"\"" } else { "'''" };
                        if line[index..].starts_with(opener) {
                            bail!(
                                "Vibe's configuration at {} contains a multiline string, which GAH cannot edit safely; set active_model by hand",
                                path.display()
                            );
                        }
                        if character == '"' {
                            in_basic = true;
                        } else {
                            in_literal = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        offset = line_feed.map_or(config_text.len(), |index| index + 1);
    }
    Ok(found)
}

/// The edit's self-verification: the edited text must parse, name exactly
/// `model` as its root `active_model`, and be otherwise identical to the
/// original's parse. Comparing the full parsed documents — with
/// `active_model` removed from both — catches collateral damage the old
/// value-only check could not: a multiline-string body overwritten by a
/// desynchronized scanner still parses, just with different contents.
fn edit_preserves_config(before: &str, after: &str, model: &str) -> bool {
    let (Ok(mut before), Ok(mut after)) = (
        toml::from_str::<toml::Value>(before),
        toml::from_str::<toml::Value>(after),
    ) else {
        return false;
    };
    let active = after.get("active_model").and_then(toml::Value::as_str);
    if active != Some(model) {
        return false;
    }
    if let (Some(before), Some(after)) = (before.as_table_mut(), after.as_table_mut()) {
        before.remove("active_model");
        after.remove("active_model");
    }
    before == after
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
    fn an_active_model_named_by_name_is_reported_by_the_alias_id() {
        // The round-2 review's case: with active_model = "devstral" and an
        // aliased "local" entry, the picker offers id "local", so the
        // active selection must be reported as "local" too — otherwise no
        // entry is marked active under an exact-id comparison.
        let selection = parse_selection(
            "active_model = \"devstral\"\n[[models]]\nname = \"devstral\"\nalias = \"local\"\n",
        )
        .unwrap();
        assert_eq!(
            selection.models,
            vec![VibeModelOption {
                id: "local".into(),
                name: "devstral".into()
            }]
        );
        assert_eq!(
            selection.active_model.as_deref(),
            Some("local"),
            "the active model is reported by the matching entry's list id"
        );
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
    fn set_active_model_refuses_configs_with_multiline_strings() {
        // The round-2 review's exact case: the `# """` comment desynced the
        // old scanner into treating the multiline body as a root key.
        let config = "# \"\"\" comment\n\
                      greeting = \"\"\"\n\
                      active_model = \"do-not-touch\"\n\
                      \"\"\"\n\
                      active_model = \"local\"\n\
                      [[models]]\n\
                      name = \"devstral\"\n\
                      alias = \"local\"\n";
        let (_guard, home) = home_with(config);
        let error = set_active_model(&home, "local").unwrap_err().to_string();
        assert!(error.contains("set active_model by hand"), "got: {error}");
        assert_eq!(
            fs::read_to_string(home.join("config.toml")).unwrap(),
            config,
            "a refused edit never touches the file"
        );
    }

    #[test]
    fn a_comment_with_quotes_is_not_treated_as_a_multiline_string() {
        // The `#` is outside any string, so the whole line — quotes
        // included — is a comment: the config is in the safe subset and
        // the edit still finds the real root key.
        let config = "# \"\"\" comment\n\
                      theme = \"auto\"\n\
                      active_model = \"do-not-touch\"\n\
                      [[models]]\n\
                      name = \"devstral\"\n\
                      alias = \"local\"\n";
        let (_guard, home) = home_with(config);
        set_active_model(&home, "local").unwrap();
        let text = fs::read_to_string(home.join("config.toml")).unwrap();
        assert_eq!(
            text,
            "# \"\"\" comment\n\
              theme = \"auto\"\n\
              active_model = \"local\"\n\
              [[models]]\n\
              name = \"devstral\"\n\
              alias = \"local\"\n",
            "only the root active_model line changes; the comment survives"
        );
    }

    #[test]
    fn set_active_model_preserves_crlf_line_endings_byte_for_byte() {
        let config = "theme = \"auto\"\r\n\
                      active_model = \"old\"\r\n\
                      [[models]]\r\n\
                      name = \"devstral\"\r\n\
                      alias = \"local\"\r\n";
        let (_guard, home) = home_with(config);
        set_active_model(&home, "local").unwrap();
        let text = fs::read_to_string(home.join("config.toml")).unwrap();
        assert_eq!(
            text,
            "theme = \"auto\"\r\n\
              active_model = \"local\"\r\n\
              [[models]]\r\n\
              name = \"devstral\"\r\n\
              alias = \"local\"\r\n",
            "the edited line keeps its CRLF; every other byte is untouched"
        );
    }

    #[test]
    fn set_active_model_prepends_with_the_files_own_crlf_line_ending() {
        let config = "theme = \"auto\"\r\n\
                      [[models]]\r\n\
                      name = \"devstral\"\r\n\
                      alias = \"local\"\r\n";
        let (_guard, home) = home_with(config);
        set_active_model(&home, "local").unwrap();
        let text = fs::read_to_string(home.join("config.toml")).unwrap();
        assert_eq!(
            text,
            "active_model = \"local\"\r\n\
              theme = \"auto\"\r\n\
              [[models]]\r\n\
              name = \"devstral\"\r\n\
              alias = \"local\"\r\n",
            "a prepended key re-uses the file's detected line ending"
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
        assert!(
            fs::read_to_string(home.join("config.toml"))
                .unwrap()
                .contains("active_model = \"mistral-vibe-cli-latest\""),
            "a name selection is persisted in the form the user chose"
        );
        assert_eq!(
            read_selection(&home).unwrap().active_model.as_deref(),
            Some("mistral-medium-3.5"),
            "the selection resolves to the covering entry's list id, like session metadata"
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
