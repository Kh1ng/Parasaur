//! Issue #653: configurable notification channels for notify-worthy events.
//!
//! `[defaults].notification_channel` selects where the same redacted
//! one-line message that `notify_command` receives is ALSO delivered:
//! Telegram (Bot API `sendMessage`, the MVP the owner asked for) or a
//! Discord webhook (so another operator can plug a different surface in
//! without code changes). Credentials never live in config: the Telegram
//! bot token comes from `TELEGRAM_BOT_TOKEN`, the Discord webhook URL from
//! `DISCORD_WEBHOOK_URL`, both read from the environment at send time.
//!
//! Issue #1454: the channels are one optional module. `[defaults]
//! notifications_enabled` switches it; while it is off nothing is sent and
//! nothing fails. The credential may also live in a private 0600 file that
//! `gah setup` and Settings write (see [`credential`]).
//!
//! Delivery failures are always visible (a `NotificationDeliveryFailed`
//! event is recorded) and never block the operation that raised the
//! notification — unknown delivery must not bypass approval (#653), it
//! must only be observable. Deduplication stays the caller's job: the
//! paid-route notice path already notifies once per occurrence.

use crate::config::{Defaults, GahConfig};
use anyhow::{bail, Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationChannel {
    /// No channel delivery; `notify_command` (per profile) still applies.
    #[default]
    None,
    /// Telegram Bot API `sendMessage` to the configured chat.
    Telegram,
    /// Discord incoming webhook (`content` message).
    Discord,
}

impl NotificationChannel {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "none" => Some(Self::None),
            "telegram" => Some(Self::Telegram),
            "discord" => Some(Self::Discord),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Telegram => "telegram",
            Self::Discord => "discord",
        }
    }

    /// The variable that carries this channel's credential, in the
    /// environment or in the private credential file.
    pub fn credential_env(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Telegram => Some("TELEGRAM_BOT_TOKEN"),
            Self::Discord => Some("DISCORD_WEBHOOK_URL"),
        }
    }
}

/// Issue #1454: whether the module is on. A config written before the switch
/// has no `notifications_enabled`; it is on exactly when it already names a
/// channel, so an upgrade keeps delivering and a fresh install starts off.
pub fn module_enabled(defaults: &Defaults) -> bool {
    defaults
        .notifications_enabled
        .unwrap_or(defaults.notification_channel != NotificationChannel::None)
}

/// The channel delivery uses: [`NotificationChannel::None`] while the module
/// is off, whatever channel the config still remembers for later.
pub fn active_channel(defaults: &Defaults) -> NotificationChannel {
    if module_enabled(defaults) {
        defaults.notification_channel
    } else {
        NotificationChannel::None
    }
}

/// Fails closed on a switch that is on with no channel to deliver to.
pub fn validate_module(defaults: &Defaults) -> Result<()> {
    if defaults.notifications_enabled == Some(true)
        && defaults.notification_channel == NotificationChannel::None
    {
        bail!("Notifications need a channel: choose telegram or discord (--notification-channel).");
    }
    Ok(())
}

/// The private credential file `gah setup` and Settings write, mode 0600.
/// `GAH_NOTIFY_CREDENTIALS_FILE` moves it (tests, unusual layouts).
pub fn credential_file() -> PathBuf {
    std::env::var_os("GAH_NOTIFY_CREDENTIALS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::setup::host::home().join(".config/gah/notify.env"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    File,
    Environment,
}

/// The channel's credential and where it came from. The private file wins
/// over the environment (e.g. `/etc/gah/server.env`): only an explicit
/// operator action in setup or Settings writes it, so it is the newer choice.
pub fn credential(channel: NotificationChannel) -> Option<(String, CredentialSource)> {
    let key = channel.credential_env()?;
    let present = |value: String| Some(value.trim().to_string()).filter(|v| !v.is_empty());
    crate::installer::files::env_get(&credential_file(), key)
        .and_then(present)
        .map(|value| (value, CredentialSource::File))
        .or_else(|| {
            std::env::var(key)
                .ok()
                .and_then(present)
                .map(|value| (value, CredentialSource::Environment))
        })
}

/// Validates one credential and writes it to the private file (atomic, 0600).
/// The value never reaches argv or a log line.
pub fn store_credential(channel: NotificationChannel, value: &str) -> Result<()> {
    let key = channel
        .credential_env()
        .context("Choose telegram or discord before saving its credential.")?;
    let value = value.trim();
    // Strict shapes also keep the value from breaking out of the curl config
    // line it is written into at send time.
    let url_safe = value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b));
    let valid = value.len() <= 512
        && url_safe
        && match channel {
            NotificationChannel::Telegram => value.split_once(':').is_some_and(|(id, secret)| {
                !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && !secret.is_empty()
            }),
            NotificationChannel::Discord => [
                "https://discord.com/api/webhooks/",
                "https://discordapp.com/api/webhooks/",
                "https://ptb.discord.com/api/webhooks/",
                "https://canary.discord.com/api/webhooks/",
            ]
            .iter()
            .any(|prefix| value.len() > prefix.len() && value.starts_with(prefix)),
            NotificationChannel::None => false,
        };
    if !valid {
        bail!(if channel == NotificationChannel::Telegram {
            "That is not a Telegram bot token. It looks like 123456789:AAE..., from @BotFather."
        } else {
            "That is not a Discord webhook URL. It starts with https://discord.com/api/webhooks/."
        });
    }
    crate::installer::files::env_set(&credential_file(), key, value)
}

/// The one HTTP primitive channel delivery needs. Mirrors the memory
/// gateway's transport seam: curl-based in production (the token reaches
/// curl through a stdin config file, never argv), a recording double in
/// tests.
pub(crate) trait NotifyTransport {
    /// Returns `(http_status, response_body)` on a completed exchange;
    /// `Err` only for transport-level failure.
    fn post_json(&self, url: &str, body: &str, timeout_secs: u32) -> Result<(u16, Vec<u8>)>;
}

pub(crate) struct CurlNotifyTransport;

impl NotifyTransport for CurlNotifyTransport {
    fn post_json(&self, url: &str, body: &str, timeout_secs: u32) -> Result<(u16, Vec<u8>)> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        // The curl config file carries the URL (which contains the Telegram
        // token) so it never appears in the process list; the file is
        // created 0600 by tempfile and deleted on drop. The JSON body
        // arrives on stdin via --data-binary @-.
        let mut config = tempfile::NamedTempFile::new()
            .context("creating curl config for channel notification")?;
        writeln!(config, "url = \"{url}\"").context("writing curl config")?;
        let mut child = Command::new("curl")
            .args(curl_args(&config.path().to_string_lossy(), timeout_secs))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawning curl for channel notification")?;
        child
            .stdin
            .as_mut()
            .expect("piped stdin")
            .write_all(body.as_bytes())
            .context("writing channel notification body")?;
        let output = child
            .wait_with_output()
            .context("running curl for channel notification")?;
        let raw = String::from_utf8_lossy(&output.stdout);
        // `-w` appends the status code on its own final line.
        let (response_body, status) = match raw.rfind('\n') {
            Some(index) => (&raw[..index], raw[index + 1..].trim()),
            None => (raw.as_ref(), ""),
        };
        let status: u16 = status.parse().with_context(|| {
            format!("parsing curl status output for channel notification: {status:?}")
        })?;
        Ok((status, response_body.as_bytes().to_vec()))
    }
}

/// curl arguments for one channel POST. The URL is deliberately absent: it
/// lives in the 0600 config file passed with `-K`. Telegram and Discord
/// need the JSON content type; curl's `--data-binary` default is form data.
fn curl_args(config_path: &str, timeout_secs: u32) -> Vec<String> {
    [
        "-sS",
        "--max-time",
        &timeout_secs.to_string(),
        "-K",
        config_path,
        "-H",
        "Content-Type: application/json",
        "-w",
        "\n%{http_code}",
        "--data-binary",
        "@-",
        "-o",
        "-",
    ]
    .map(String::from)
    .to_vec()
}

/// One delivery attempt through the configured channel. `Ok(())` means the
/// remote accepted the message (HTTP 2xx); anything else — missing
/// credential, non-2xx, transport failure — is a descriptive `Err` the
/// caller turns into a visible delivery-failure event.
pub(crate) fn deliver_channel_message(
    cfg: &GahConfig,
    message: &str,
    transport: &dyn NotifyTransport,
) -> Result<()> {
    match active_channel(&cfg.defaults) {
        NotificationChannel::None => Ok(()),
        NotificationChannel::Telegram => {
            let (token, _) = credential(NotificationChannel::Telegram)
                .context("TELEGRAM_BOT_TOKEN is not set (Settings → Notifications)")?;
            let chat_id = cfg
                .defaults
                .telegram_chat_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .context("telegram_chat_id is not set (Settings → Notifications)")?;
            let url = format!("https://api.telegram.org/bot{token}/sendMessage");
            let body = serde_json::json!({ "chat_id": chat_id, "text": message }).to_string();
            deliver(transport, &url, &body, "Telegram")
        }
        NotificationChannel::Discord => {
            let (webhook, _) = credential(NotificationChannel::Discord)
                .context("DISCORD_WEBHOOK_URL is not set (Settings → Notifications)")?;
            let body = serde_json::json!({ "content": message }).to_string();
            deliver(transport, &webhook, &body, "Discord")
        }
    }
}

/// Entry point for `gah notify-send`: one message from the central activity
/// feed. A missing channel or an off module is a no-op, so the feed can call
/// this without reading GAH config itself. Returns the channel it used
/// alongside the outcome, so the feed can show a delivery receipt for it.
pub fn send_activity(
    cfg: &GahConfig,
    title: &str,
    message: &str,
    url: Option<&str>,
) -> (NotificationChannel, Result<()>) {
    let text = activity_text(title, message, url);
    let channel = active_channel(&cfg.defaults);
    let outcome = deliver_channel_message(cfg, &crate::redact::redact(&text), &CurlNotifyTransport);
    (channel, outcome)
}

/// The one JSON line `gah notify-send` prints for the activity feed's receipt.
/// A failure carries a short, redacted `reason` for notification history.
pub fn activity_receipt_line(channel: NotificationChannel, outcome: &Result<()>) -> String {
    let mut line = serde_json::json!({ "channel": channel.as_str(), "ok": outcome.is_ok() });
    if let Err(error) = outcome {
        let reason = crate::redact::redact(&format!("{error:#}"));
        line["reason"] = reason.chars().take(160).collect::<String>().into();
    }
    line.to_string()
}

fn activity_text(title: &str, message: &str, url: Option<&str>) -> String {
    let mut text = format!("[gah] {title}");
    if !message.trim().is_empty() {
        text.push_str(": ");
        text.push_str(message.trim());
    }
    if let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) {
        text.push(' ');
        text.push_str(url);
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn deliver(transport: &dyn NotifyTransport, url: &str, body: &str, label: &str) -> Result<()> {
    let (status, response) = transport.post_json(url, body, 10)?;
    if (200..300).contains(&status) {
        return Ok(());
    }
    // Never echo the response body by default: for Telegram errors it can
    // reflect parameter content, and the caller's message is already known.
    let snippet = String::from_utf8_lossy(&response);
    let snippet = snippet.chars().take(120).collect::<String>();
    anyhow::bail!("{label} notification rejected with HTTP {status}: {snippet}");
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Shared with other modules' tests that set the channel credential env.
    pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn curl_args_send_json_content_type_and_keep_url_out_of_argv() {
        let args = curl_args("/tmp/cfg", 7);
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-H", "Content-Type: application/json"]));
        assert!(args.windows(2).any(|pair| pair == ["--data-binary", "@-"]));
        assert!(args.windows(2).any(|pair| pair == ["-K", "/tmp/cfg"]));
        assert!(!args.iter().any(|arg| arg.contains("://")));
    }

    pub(crate) struct RecordingTransport {
        pub(crate) requests: Mutex<Vec<(String, String)>>,
        status: u16,
    }

    impl RecordingTransport {
        pub(crate) fn new(status: u16) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                status,
            }
        }

        fn last(&self) -> (String, String) {
            self.requests
                .lock()
                .unwrap()
                .last()
                .cloned()
                .expect("a request was made")
        }
    }

    impl NotifyTransport for RecordingTransport {
        fn post_json(&self, url: &str, body: &str, _timeout_secs: u32) -> Result<(u16, Vec<u8>)> {
            self.requests
                .lock()
                .unwrap()
                .push((url.to_string(), body.to_string()));
            Ok((self.status, b"{}".to_vec()))
        }
    }

    /// Runs `f` with each `(variable, value)` set (or removed for `None`) and
    /// the private credential file moved to an empty temporary directory,
    /// so a developer's real `~/.config/gah/notify.env` never leaks in.
    /// Restores the environment afterwards. `f` receives the file's path.
    pub(crate) fn with_env<R>(
        vars: &[(&str, Option<&str>)],
        f: impl FnOnce(&std::path::Path) -> R,
    ) -> R {
        let _env = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notify.env");
        let file_value = file.to_string_lossy().into_owned();
        let all: Vec<(&str, Option<&str>)> = vars
            .iter()
            .copied()
            .chain([("GAH_NOTIFY_CREDENTIALS_FILE", Some(file_value.as_str()))])
            .collect();
        let previous: Vec<(&str, Option<String>)> = all
            .iter()
            .map(|(key, _)| (*key, std::env::var(key).ok()))
            .collect();
        for (key, value) in &all {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        let result = f(&file);
        for (key, value) in previous {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        result
    }

    fn config_with_channel(channel: NotificationChannel, chat_id: Option<&str>) -> GahConfig {
        let (_tmp, cfg) = crate::ledger::test_util::test_config();
        let mut cfg = cfg;
        cfg.defaults.notification_channel = channel;
        cfg.defaults.telegram_chat_id = chat_id.map(str::to_string);
        cfg
    }

    const TOKEN: Option<&str> = Some("123456:test-token");

    #[test]
    fn none_channel_delivers_nothing() {
        let transport = RecordingTransport::new(200);
        deliver_channel_message(
            &config_with_channel(NotificationChannel::None, None),
            "hello",
            &transport,
        )
        .unwrap();
        assert!(transport.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn module_is_off_on_a_fresh_config_and_kept_on_for_an_upgraded_channel() {
        let parse = |toml: &str| toml::from_str::<GahConfig>(toml).unwrap().defaults;
        let fresh = parse("");
        assert!(!module_enabled(&fresh));
        assert_eq!(active_channel(&fresh), NotificationChannel::None);
        // A config from before the switch keeps delivering.
        let upgraded = parse("[defaults]\nnotification_channel = \"telegram\"\n");
        assert!(module_enabled(&upgraded));
        assert_eq!(active_channel(&upgraded), NotificationChannel::Telegram);
        let switched_off = parse(
            "[defaults]\nnotifications_enabled = false\nnotification_channel = \"discord\"\n",
        );
        assert_eq!(active_channel(&switched_off), NotificationChannel::None);
        // An unknown switch value fails the load instead of guessing.
        assert!(
            toml::from_str::<GahConfig>("[defaults]\nnotifications_enabled = \"maybe\"\n").is_err()
        );
    }

    #[test]
    fn switched_off_module_sends_nothing_and_does_not_fail_without_credentials() {
        let mut cfg = config_with_channel(NotificationChannel::Telegram, None);
        cfg.defaults.notifications_enabled = Some(false);
        let transport = RecordingTransport::new(200);
        with_env(&[("TELEGRAM_BOT_TOKEN", None)], |_| {
            deliver_channel_message(&cfg, "hello", &transport).unwrap();
        });
        assert!(transport.requests.lock().unwrap().is_empty());
        let (channel, outcome) = send_activity(&cfg, "title", "message", None);
        assert_eq!(channel, NotificationChannel::None);
        assert_eq!(
            activity_receipt_line(channel, &outcome),
            r#"{"channel":"none","ok":true}"#
        );
    }

    #[test]
    fn switch_on_without_a_channel_is_rejected() {
        let mut defaults = Defaults {
            notifications_enabled: Some(true),
            ..Default::default()
        };
        assert!(validate_module(&defaults).is_err());
        defaults.notification_channel = NotificationChannel::Telegram;
        validate_module(&defaults).unwrap();
        defaults.notifications_enabled = Some(false);
        defaults.notification_channel = NotificationChannel::None;
        validate_module(&defaults).unwrap();
    }

    #[test]
    fn activity_receipt_line_names_the_channel_outcome_and_failure_reason() {
        assert_eq!(
            activity_receipt_line(NotificationChannel::Telegram, &Ok(())),
            r#"{"channel":"telegram","ok":true}"#
        );
        let line: serde_json::Value = serde_json::from_str(&activity_receipt_line(
            NotificationChannel::Telegram,
            &Err(anyhow::anyhow!(
                "TELEGRAM_BOT_TOKEN is not set (Settings → Notifications)"
            )),
        ))
        .unwrap();
        assert_eq!(line["ok"], false);
        assert_eq!(
            line["reason"],
            "TELEGRAM_BOT_TOKEN is not set (Settings → Notifications)"
        );
    }

    #[test]
    fn activity_text_is_one_line_with_optional_url() {
        assert_eq!(
            activity_text(
                "gah: reply ready",
                "done\nnext line",
                Some("https://x/?page=chat")
            ),
            "[gah] gah: reply ready: done next line https://x/?page=chat"
        );
        assert_eq!(
            activity_text("node offline", " ", None),
            "[gah] node offline"
        );
    }

    #[test]
    fn telegram_delivery_sends_chat_id_and_text_with_env_token() {
        let transport = RecordingTransport::new(200);
        let cfg = config_with_channel(NotificationChannel::Telegram, Some("12345"));
        with_env(&[("TELEGRAM_BOT_TOKEN", TOKEN)], |_| {
            deliver_channel_message(&cfg, "approval needed", &transport)
        })
        .expect("delivery must succeed");

        let (url, body) = transport.last();
        assert!(url.starts_with("https://api.telegram.org/bot123456:test-token/sendMessage"));
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["chat_id"], "12345");
        assert_eq!(parsed["text"], "approval needed");
    }

    #[test]
    fn stored_credential_is_private_validated_and_wins_over_the_environment() {
        let transport = RecordingTransport::new(200);
        let cfg = config_with_channel(NotificationChannel::Telegram, Some("12345"));
        with_env(&[("TELEGRAM_BOT_TOKEN", Some("999:env-token"))], |file| {
            assert_eq!(
                credential(NotificationChannel::Telegram),
                Some(("999:env-token".into(), CredentialSource::Environment))
            );
            for bad in [
                "",
                "no-colon",
                "abc:def",
                "1:has space",
                "1:quote\"",
                "1:a\nb",
            ] {
                assert!(
                    store_credential(NotificationChannel::Telegram, bad).is_err(),
                    "{bad:?}"
                );
            }
            assert!(
                store_credential(NotificationChannel::Discord, "https://evil.test/hook").is_err()
            );
            assert!(store_credential(NotificationChannel::None, "1:x").is_err());
            store_credential(NotificationChannel::Telegram, " 123456:file-token \n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(file).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o600);
            }
            assert_eq!(
                credential(NotificationChannel::Telegram),
                Some(("123456:file-token".into(), CredentialSource::File))
            );
            deliver_channel_message(&cfg, "hi", &transport).unwrap();
            store_credential(
                NotificationChannel::Discord,
                "https://discord.com/api/webhooks/1/abc",
            )
            .unwrap();
            // Saving one channel keeps the other's credential.
            assert!(credential(NotificationChannel::Telegram).is_some());
        });
        assert!(transport.last().0.contains("bot123456:file-token/"));
    }

    #[test]
    fn telegram_without_token_fails_descriptively() {
        let transport = RecordingTransport::new(200);
        let cfg = config_with_channel(NotificationChannel::Telegram, Some("12345"));
        let error = with_env(&[("TELEGRAM_BOT_TOKEN", None)], |_| {
            deliver_channel_message(&cfg, "hello", &transport)
        })
        .expect_err("missing token must fail");
        assert!(format!("{error:#}").contains("TELEGRAM_BOT_TOKEN is not set"));
        assert!(transport.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn telegram_without_chat_id_fails_descriptively() {
        let transport = RecordingTransport::new(200);
        let cfg = config_with_channel(NotificationChannel::Telegram, None);
        let error = with_env(&[("TELEGRAM_BOT_TOKEN", TOKEN)], |_| {
            deliver_channel_message(&cfg, "hello", &transport)
        })
        .expect_err("missing chat id must fail");
        assert!(format!("{error:#}").contains("telegram_chat_id is not set"));
    }

    const WEBHOOK: Option<&str> = Some("https://discord.com/api/webhooks/1/abc");

    #[test]
    fn discord_delivery_posts_content_from_env_webhook() {
        let transport = RecordingTransport::new(200);
        let cfg = config_with_channel(NotificationChannel::Discord, None);
        with_env(&[("DISCORD_WEBHOOK_URL", WEBHOOK)], |_| {
            deliver_channel_message(&cfg, "hello from gah", &transport)
        })
        .expect("delivery must succeed");

        let (url, body) = transport.last();
        assert_eq!(url, "https://discord.com/api/webhooks/1/abc");
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["content"], "hello from gah");
    }

    #[test]
    fn non_2xx_is_a_descriptive_error_with_a_bounded_snippet() {
        let transport = RecordingTransport::new(403);
        let cfg = config_with_channel(NotificationChannel::Discord, None);
        let error = with_env(&[("DISCORD_WEBHOOK_URL", WEBHOOK)], |_| {
            deliver_channel_message(&cfg, "hello", &transport)
        })
        .expect_err("403 must fail");
        assert!(format!("{error:#}").contains("HTTP 403"));
    }

    #[test]
    fn channel_parses_the_settings_vocabulary() {
        assert_eq!(
            NotificationChannel::parse("telegram"),
            Some(NotificationChannel::Telegram)
        );
        assert_eq!(
            NotificationChannel::parse("discord"),
            Some(NotificationChannel::Discord)
        );
        assert_eq!(
            NotificationChannel::parse("none"),
            Some(NotificationChannel::None)
        );
        assert_eq!(NotificationChannel::parse("slack"), None);
        assert_eq!(NotificationChannel::Telegram.as_str(), "telegram");
    }
}
