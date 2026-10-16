//! Plain Mistral API keys cannot read admin or dashboard billing. A one-token
//! completion is the only key-scoped signal: 402 means its budget is spent.
use anyhow::{bail, Context, Result};
use std::process::Command;

/// `true` when the key's Vibe or API budget is exhausted. The key reaches
/// curl through its stdin config, never argv.
pub(crate) fn budget_exhausted(key: &str) -> Result<bool> {
    if key.is_empty() || key.len() > 8192 || key.chars().any(char::is_control) {
        bail!("invalid Mistral credential");
    }
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    let mut command = Command::new("curl");
    command.args([
        "--disable",
        "--silent",
        "--max-time",
        "15",
        "--max-filesize",
        "524288",
        "--write-out",
        "\n%{http_code}",
        "-K",
        "-",
    ]);
    crate::runner::process::arm_child_pdeathsig(&mut command);
    let config = format!(
        "url = \"https://api.mistral.ai/v1/chat/completions\"\nheader = \"Authorization: Bearer {escaped}\"\nheader = \"Content-Type: application/json\"\nheader = \"Accept: application/json\"\ndata = \"{{\\\"model\\\":\\\"mistral-vibe-cli-latest\\\",\\\"max_tokens\\\":1,\\\"messages\\\":[{{\\\"role\\\":\\\"user\\\",\\\"content\\\":\\\"hi\\\"}}]}}\"\n"
    );
    let output = crate::runner::process::run_bounded_with_input(
        command,
        std::time::Duration::from_secs(20),
        config.as_bytes(),
    )
    .context("Mistral budget check failed or timed out")?;
    if !output.status.success() {
        bail!("Mistral budget check failed or timed out");
    }
    let split = output
        .stdout
        .iter()
        .rposition(|byte| *byte == b'\n')
        .context("invalid Mistral HTTP response")?;
    parse(&output.stdout[split + 1..], &output.stdout[..split])
}

fn parse(status: &[u8], body: &[u8]) -> Result<bool> {
    match status {
        b"200" => Ok(false),
        b"401" | b"403" => bail!("auth_required: Mistral rejected the API key"),
        b"402" => {
            let body: serde_json::Value =
                serde_json::from_slice(body).context("invalid Mistral billing response")?;
            match body["type"].as_str() {
                Some("billing_vibe_budget_exhausted" | "billing_api_budget_exhausted") => Ok(true),
                _ => bail!("unknown Mistral billing response"),
            }
        }
        _ => bail!("Mistral budget check HTTP request failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn probe_statuses() {
        let vibe = br#"{"object":"error","message":"budget exhausted","type":"billing_vibe_budget_exhausted","param":null,"code":"2304"}"#;
        let api = br#"{"object":"error","message":"budget exhausted","type":"billing_api_budget_exhausted","param":null,"code":"2303"}"#;
        assert!(parse(b"402", vibe).unwrap());
        assert!(parse(b"402", api).unwrap());
        assert!(!parse(b"200", br#"{"id":"x","choices":[]}"#).unwrap());
        let auth = parse(b"401", br#"{"message":"Unauthorized"}"#).unwrap_err();
        assert!(auth.to_string().starts_with("auth_required:"));
        assert!(parse(b"402", br#"{"type":"other"}"#).is_err());
        assert!(parse(b"429", b"{}").is_err());
    }
}
