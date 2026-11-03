//! The browser stays native; verification and persistence belong to the WSL node.
use super::{status, LoginStatus, LoginTarget};
use crate::credentials::windows::{run, wsl_command};
use std::time::Duration;

pub(super) fn installed() -> bool {
    let command = wsl_command(
        &crate::read_settings(),
        "/bin/sh",
        vec![
            "-c".into(),
            "test -x \"$HOME/.cargo/bin/gah\" && command -v python3 >/dev/null".into(),
        ],
        Duration::from_secs(10),
    );
    run(command, None, Duration::from_secs(13)).is_ok()
}

pub(super) fn verify_and_save(cookie: &str, target: &LoginTarget) -> LoginStatus {
    let command = check_command(&crate::read_settings(), target, false);
    let result = run(command, Some(cookie.as_bytes()), Duration::from_secs(198))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    match result.as_ref().and_then(|value| value["result"].as_str()) {
        Some("connected") => status("connected", true, "Mistral is connected on this computer. Usage and allowance are refreshed automatically."),
        Some("not_installed") => status("unavailable", false, "Install Parasaur in the selected WSL distribution before connecting Mistral."),
        Some("custom_source") => super::custom_cookie_status(),
        Some("auth_required" | "invalid_cookie") => status("pending", true, "Mistral has not accepted this session. Finish signing in and check the connection."),
        Some("usage") => status("unavailable", true, "Mistral is saved, but its usage check failed. Check this connection again."),
        Some("publish") => status("unavailable", true, "Mistral is connected, but its usage could not be published. Check the connection again."),
        _ => status("unavailable", true, "Mistral usage could not be verified. Check the connection again or update Parasaur in WSL."),
    }
}

pub(super) fn preflight(target: &LoginTarget) -> Result<(), LoginStatus> {
    let command = check_command(&crate::read_settings(), target, true);
    let result = run(command, None, Duration::from_secs(13))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    match result.as_ref().and_then(|value| value["result"].as_str()) {
        Some("ready") => Ok(()),
        Some("custom_source") => Err(super::custom_cookie_status()),
        _ => Err(status(
            "unavailable",
            false,
            "Mistral sign-in requires Parasaur and Python in the selected WSL distribution.",
        )),
    }
}

fn check_command(
    settings: &crate::DesktopSettings,
    target: &LoginTarget,
    preflight: bool,
) -> std::process::Command {
    wsl_command(
        settings,
        "/usr/bin/python3",
        vec![
            "-c".into(),
            include_str!("windows_check.py").into(),
            if preflight { "preflight" } else { "verify" }.into(),
            target.credential_id.clone().unwrap_or_default(),
            target.account_label.clone().unwrap_or_default(),
        ],
        Duration::from_secs(if preflight { 10 } else { 195 }),
    )
}

#[cfg(test)]
mod browser_test;
#[cfg(test)]
mod tests;
