//! Development application variables for interactive agents. Provider identity,
//! control-plane settings, and work-scoped credentials remain separate grants.
use crate::config::Profile;
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

const MAX_BYTES: u64 = 128 * 1024;

fn application_key(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    ![
        "GAH_",
        "COORDINATOR_",
        "TDAI_",
        "TELEGRAM_",
        "DISCORD_",
        "XDG_",
        "OPENAI_",
        "ANTHROPIC_",
        "CODEX_",
        "CLAUDE_",
        "HERMES_",
        "VIBE_",
        "OPENCODE_",
        "OPENHANDS_",
        "LLM_",
        "GOOGLE_",
        "GEMINI_",
        "MISTRAL_",
        "NOUS_",
        "XAI_",
        "MOONSHOT_",
        "DEEPSEEK_",
        "OPENROUTER_",
        "GROQ_",
        "TOGETHER_",
        "CEREBRAS_",
        "AWS_",
        "AZURE_",
        "LD_",
        "DYLD_",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        && !matches!(
            name.as_str(),
            "HOME"
                | "USERPROFILE"
                | "APPDATA"
                | "LOCALAPPDATA"
                | "HOMEDRIVE"
                | "HOMEPATH"
                | "NODE_OPTIONS"
                | "MODEL_PROVIDER"
                | "DEFAULT_AUTH_REQUEST"
                | "GITHUB_TOKEN"
                | "GH_TOKEN"
                | "GITLAB_PAT"
                | "GITLAB_PAT2"
        )
}

/// Only development is available to chat. Resolve relative sources against the
/// configured checkout, not a conversation's disposable worktree. Never export
/// these values to the parent process or include file/source text in errors.
pub(crate) fn load(profile: &Profile) -> Result<BTreeMap<String, String>> {
    let Some(source) = profile
        .env_file
        .as_deref()
        .filter(|source| !source.is_empty())
    else {
        return Ok(BTreeMap::new());
    };
    let source = Path::new(source);
    let path = if source.is_absolute() {
        source.to_owned()
    } else {
        Path::new(&profile.local_path).join(source)
    };
    let read = || -> Result<String> {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = options.open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES {
            bail!("invalid workspace environment source");
        }
        let mut text = String::new();
        file.take(MAX_BYTES + 1).read_to_string(&mut text)?;
        if text.len() as u64 > MAX_BYTES {
            bail!("workspace environment source is too large");
        }
        Ok(text)
    };
    let text = read()
        .map_err(|_| anyhow::anyhow!("Cannot read the configured development environment file."))?;
    let mut result = BTreeMap::new();
    for (name, value) in crate::runner::parse_env_file(&text) {
        if name.is_empty()
            || !name.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || index > 0 && byte.is_ascii_digit()
            })
            || value.contains('\0')
        {
            bail!("The development environment file contains an invalid assignment.");
        }
        if application_key(&name)
            && !profile.external_credential_scopes.values().any(|scope| {
                scope
                    .env_vars
                    .iter()
                    .any(|scoped| scoped.eq_ignore_ascii_case(&name))
            })
        {
            result.insert(name, value);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(path: &Path) -> Profile {
        serde_json::from_value(serde_json::json!({
            "display_name": "Workspace", "repo_id": "owner/repo", "provider": "github",
            "repo": "owner/repo", "local_path": path, "artifact_root": path.join("artifacts"),
            "default_target_branch": "main"
        }))
        .unwrap()
    }

    #[test]
    fn development_variables_are_private_and_do_not_replace_accounts_or_grants() {
        let temp = tempfile::tempdir().unwrap();
        let mut profile = profile(temp.path());
        std::fs::write(temp.path().join("dev.env"), concat!(
            "# same parser as dispatch\nAPP_URL=\"http://localhost:3000/a=b\"\nAPP_LABEL='work space'\n",
            "PATH=/workspace/bin\nOPENAI_API_KEY=wrong-account\nHOME=/wrong/account\nhOmE=/wrong/windows-account\n",
            "GAH_CONFIG_PATH=/wrong/config\nCOORDINATOR_TOKEN=control-secret\nTELEGRAM_BOT_TOKEN=bot-secret\n",
            "CLAUDE_CODE_OAUTH_TOKEN=wrong-subscription\nNODE_OPTIONS=--require=other.js\n",
            "XDG_CONFIG_HOME=/wrong/home\nAPP_LABEL=last value\n"
        )).unwrap();
        std::fs::write(
            temp.path().join("prod.env"),
            "PRODUCTION_ONLY=must-not-load\n",
        )
        .unwrap();
        profile.env_file = Some("dev.env".into());
        profile.env_file_prod = Some("prod.env".into());
        profile.external_credential_scopes.insert(
            "external-api".into(),
            crate::config::ExternalCredentialScope {
                env_vars: vec!["SCOPED_APP_KEY".into()],
                ..Default::default()
            },
        );
        use std::io::Write;
        writeln!(
            std::fs::OpenOptions::new()
                .append(true)
                .open(temp.path().join("dev.env"))
                .unwrap(),
            "SCOPED_APP_KEY=not-approved"
        )
        .unwrap();
        let loaded = load(&profile).unwrap();
        assert_eq!(
            loaded,
            BTreeMap::from([
                ("APP_URL".into(), "http://localhost:3000/a=b".into()),
                ("APP_LABEL".into(), "last value".into()),
                ("PATH".into(), "/workspace/bin".into()),
            ])
        );
        profile.env_file = None;
        assert!(
            load(&profile).unwrap().is_empty(),
            "production is never a fallback"
        );
    }

    #[test]
    fn configured_missing_invalid_and_oversized_sources_fail_without_source_text() {
        let temp = tempfile::tempdir().unwrap();
        let mut profile = profile(temp.path());
        profile.env_file = Some("PRIVATE_PATH_CANARY".into());
        let error = load(&profile).unwrap_err().to_string();
        assert!(!error.contains("PRIVATE_PATH_CANARY"));
        std::fs::write(
            temp.path().join("PRIVATE_PATH_CANARY"),
            "BAD-KEY=PRIVATE_VALUE_CANARY",
        )
        .unwrap();
        let error = load(&profile).unwrap_err().to_string();
        assert!(!error.contains("PRIVATE_VALUE_CANARY"));
        std::fs::write(
            temp.path().join("PRIVATE_PATH_CANARY"),
            vec![b'x'; MAX_BYTES as usize + 1],
        )
        .unwrap();
        assert!(load(&profile).is_err());
        profile.env_file = Some(temp.path().to_string_lossy().into_owned());
        assert!(
            load(&profile).is_err(),
            "directories are not environment files"
        );
    }
}
