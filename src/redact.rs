//! Central, conservative secret redaction for every durable or external sink.
//!
//! This is intentionally on by by default.  Callers should pass text through
//! [`redact`] before writing an artifact, ledger/event record, provider body,
//! or notification.  The function is idempotent, so applying it at more than
//! one boundary is safe and gives individual sinks a useful last line of
//! defense.

use regex::Regex;
use serde_json::Value;
use std::sync::{OnceLock, RwLock};

/// Values shorter than this are never treated as secrets, so ordinary prose
/// is not replaced accidentally.
const MIN_SECRET_LEN: usize = 4;

/// From this length on a known value is redacted wherever it appears, even
/// inside a longer identifier: such a value is not an ordinary word, and any
/// verbatim occurrence of it is a disclosure.
const UNANCHORED_SECRET_LEN: usize = 16;

fn github_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\b(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,})\b")
            .expect("valid GitHub token regex")
    })
}

fn gitlab_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\bglpat-[A-Za-z0-9_-]{20,}\b").expect("valid GitLab token regex")
    })
}

fn api_key_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bsk-(?:[A-Za-z0-9_-]{20,})\b").expect("valid API key regex"))
}

fn bearer_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?i)(authorization\s*:\s*bearer\s+)[^\s\"']+"#)
            .expect("valid bearer token regex")
    })
}

fn basic_auth_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(https?://)[^/\s@]+@").expect("valid basic-auth URL regex"))
}

fn query_secret_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)([?&](?:access_token|api[_-]?key|token|password)=)[^&#\s]+")
            .expect("valid query secret regex")
    })
}

fn secret_env_values() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(name, value)| is_secret_env_name(name) && value.len() >= MIN_SECRET_LEN)
        .collect()
}

fn secret_registry() -> &'static RwLock<Vec<(String, String)>> {
    static REGISTRY: OnceLock<RwLock<Vec<(String, String)>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(Vec::new()))
}

/// Register a secret this process holds outside its own environment, such as
/// a named credential injected only into a child's environment.
///
/// Every later [`redact`] and [`redact_json_value`] call in this process
/// replaces the value with `[REDACTED:<name>]`. Values shorter than the
/// env-derived minimum are ignored, and a value already registered keeps its
/// first name. Registration is never undone: a rotated-out value is still a
/// secret.
pub fn register_secret(name: &str, value: &str) {
    if value.len() < MIN_SECRET_LEN {
        return;
    }
    // A poisoned lock still holds a usable list; redaction must not stop.
    let mut secrets = secret_registry()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if secrets.iter().any(|(_, known)| known == value) {
        return;
    }
    secrets.push((name.to_owned(), value.to_owned()));
}

/// Live environment secrets plus every registered secret, read at call time
/// so a newly registered or newly exported value is covered immediately.
fn known_secrets() -> Vec<(String, String)> {
    let mut secrets = secret_env_values();
    secrets.extend(
        secret_registry()
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned(),
    );
    secrets
}

fn word_char_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\w$").expect("valid word character regex"))
}

/// Whether `\b` can match beside `ch`: the regex engine's own (Unicode) word
/// class decides, so the anchor and this check never disagree.
fn is_word_char(ch: char) -> bool {
    let mut buf = [0_u8; 4];
    word_char_re().is_match(ch.encode_utf8(&mut buf))
}

/// Pattern for one known value. A short value is anchored with `\b` only on
/// an edge whose character is a word character: a value that starts or ends
/// with punctuation (`hunter2!`, base64 padding) has no word boundary there.
/// A long value is matched anywhere.
fn known_secret_pattern(value: &str) -> Option<Regex> {
    let escaped = regex::escape(value);
    if value.len() >= UNANCHORED_SECRET_LEN {
        return Regex::new(&escaped).ok();
    }
    let first = value.chars().next()?;
    let last = value.chars().next_back()?;
    let start = if is_word_char(first) { r"\b" } else { "" };
    let end = if is_word_char(last) { r"\b" } else { "" };
    Regex::new(&format!("{start}{escaped}{end}")).ok()
}

fn is_secret_env_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "API_KEY",
        "PRIVATE_KEY",
    ]
    .iter()
    .any(|needle| name.contains(needle))
}

/// Redact known live environment secrets, registered secrets (see
/// [`register_secret`]) and common provider-token forms.
pub fn redact(text: &str) -> String {
    redact_with_known_secrets(text, &known_secrets())
}

/// Redact text using explicit secrets as well as provider token patterns.
///
/// Exposed primarily for deterministic unit tests and callers that hold a
/// credential outside the process environment.  A short secret is ignored to
/// avoid replacing ordinary prose accidentally.
pub fn redact_with_known_secrets(text: &str, secrets: &[(String, String)]) -> String {
    let mut output = text.to_string();
    let mut secrets: Vec<_> = secrets
        .iter()
        .filter(|(_, value)| value.len() >= MIN_SECRET_LEN)
        .collect();
    // Longest-first prevents a prefix secret from leaving the suffix behind.
    secrets.sort_by_key(|secret| std::cmp::Reverse(secret.1.len()));
    for (name, value) in secrets {
        // A raw substring replace corrupts unrelated text whenever a short
        // secret value (e.g. a test harness's `GITHUB_TOKEN=token`, or any
        // real value that is coincidentally a short common word) also
        // appears inside a longer, unrelated word -- "token" inside
        // "input_tokens" being the concrete case that surfaced this. Anchor
        // on word boundaries so only a standalone occurrence of the value is
        // redacted, never a substring of a different identifier. Only edges
        // that are word characters can carry a boundary, and a long value is
        // never a common word (see `known_secret_pattern`).
        let Some(pattern) = known_secret_pattern(value) else {
            continue;
        };
        output = pattern
            .replace_all(&output, regex::NoExpand(&format!("[REDACTED:{name}]")))
            .into_owned();
    }
    output = github_token_re()
        .replace_all(&output, "[REDACTED:GITHUB_TOKEN]")
        .into_owned();
    output = gitlab_token_re()
        .replace_all(&output, "[REDACTED:GITLAB_TOKEN]")
        .into_owned();
    output = api_key_re()
        .replace_all(&output, "[REDACTED:API_KEY]")
        .into_owned();
    output = bearer_re()
        .replace_all(&output, "$1[REDACTED:TOKEN]")
        .into_owned();
    output = basic_auth_url_re()
        .replace_all(&output, "$1[REDACTED:URL_CREDENTIAL]@")
        .into_owned();
    query_secret_re()
        .replace_all(&output, "$1[REDACTED:URL_CREDENTIAL]")
        .into_owned()
}

/// Recursively redact every string in a JSON value before serialization.
pub fn redact_json_value(value: &mut Value) {
    redact_json_value_with_known_secrets(value, &known_secrets());
}

fn redact_json_value_with_known_secrets(value: &mut Value, secrets: &[(String, String)]) {
    match value {
        Value::String(text) => *text = redact_with_known_secrets(text, secrets),
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| redact_json_value_with_known_secrets(value, secrets)),
        Value::Object(values) => values
            .values_mut()
            .for_each(|value| redact_json_value_with_known_secrets(value, secrets)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{redact_json_value_with_known_secrets, redact_with_known_secrets, register_secret};

    fn redact(text: &str) -> String {
        redact_with_known_secrets(
            text,
            &[("DEMO_TOKEN".to_string(), "s3cr3t-value-123".to_string())],
        )
    }

    #[test]
    fn short_known_secret_value_does_not_corrupt_a_longer_unrelated_word() {
        // Regression: a test harness (or a real deployment) can set a short
        // *_TOKEN env value like "token". A raw substring replace turned
        // "input_tokens: 500" into "input_[REDACTED:...]s: 500", silently
        // destroying the field name and losing real usage data downstream.
        let result = redact_with_known_secrets(
            "input_tokens: 500\noutput_tokens: 120",
            &[("GITHUB_TOKEN".to_string(), "token".to_string())],
        );
        assert_eq!(result, "input_tokens: 500\noutput_tokens: 120");
    }

    #[test]
    fn short_known_secret_value_is_still_redacted_as_a_standalone_word() {
        let result = redact_with_known_secrets(
            "auth failed: token invalid",
            &[("GITHUB_TOKEN".to_string(), "token".to_string())],
        );
        assert_eq!(result, "auth failed: [REDACTED:GITHUB_TOKEN] invalid");
    }

    #[test]
    fn known_secret_ending_in_punctuation_is_redacted() {
        let result = redact_with_known_secrets(
            "login with hunter2! failed",
            &[("DB_PASSWORD".to_string(), "hunter2!".to_string())],
        );
        assert_eq!(result, "login with [REDACTED:DB_PASSWORD] failed");
    }

    #[test]
    fn known_base64_secret_with_padding_is_redacted() {
        let secrets = [
            ("SHORT_SECRET".to_string(), "YWJjZA==".to_string()),
            (
                "LONG_SECRET".to_string(),
                "c2VjcmV0LXZhbHVlLTEy==".to_string(),
            ),
        ];
        let result = redact_with_known_secrets("a=YWJjZA== b=c2VjcmV0LXZhbHVlLTEy== end", &secrets);
        assert_eq!(
            result,
            "a=[REDACTED:SHORT_SECRET] b=[REDACTED:LONG_SECRET] end"
        );
    }

    #[test]
    fn known_secret_directly_after_word_characters_is_redacted() {
        let result = redact_with_known_secrets(
            "POST https://api.example.test/bot123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw/sendMessage",
            &[(
                "BOT_TOKEN".to_string(),
                "123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw".to_string(),
            )],
        );
        assert_eq!(
            result,
            "POST https://api.example.test/bot[REDACTED:BOT_TOKEN]/sendMessage"
        );
    }

    #[test]
    fn registered_secret_outside_process_env_is_redacted() {
        let value = "registered-only-value-4f1c9e";
        assert!(std::env::vars().all(|(_, env_value)| env_value != value));
        register_secret("SELECTED_API_KEY", value);
        register_secret("IGNORED_SHORT", "abc");
        assert_eq!(
            super::redact(&format!("child echoed {value}; abc")),
            "child echoed [REDACTED:SELECTED_API_KEY]; abc"
        );
        let mut json = serde_json::json!({"nested": [format!("key {value}")]});
        super::redact_json_value(&mut json);
        assert_eq!(json["nested"][0], "key [REDACTED:SELECTED_API_KEY]");
    }

    #[test]
    fn redacts_known_values_and_provider_patterns_without_losing_context() {
        let result = redact(
            "failed with s3cr3t-value-123; glpat-abcdefghijklmnopqrstuv; Authorization: Bearer abcdefghijklmnopqrstuvwxyz",
        );
        assert_eq!(
            result,
            "failed with [REDACTED:DEMO_TOKEN]; [REDACTED:GITLAB_TOKEN]; Authorization: Bearer [REDACTED:TOKEN]"
        );
    }

    #[test]
    fn redacts_github_api_and_basic_auth_url_tokens() {
        let result = redact(
            "ghp_abcdefghijklmnopqrstuvwxyz https://user:pass@example.test/a?access_token=abcdefghi",
        );
        assert_eq!(
            result,
            "[REDACTED:GITHUB_TOKEN] https://[REDACTED:URL_CREDENTIAL]@example.test/a?access_token=[REDACTED:URL_CREDENTIAL]"
        );
    }

    #[test]
    fn is_idempotent_and_preserves_ordinary_text() {
        let once = redact("ordinary output: cargo test passed");
        assert_eq!(once, "ordinary output: cargo test passed");
        assert_eq!(redact(&once), once);
    }

    #[test]
    fn redacts_strings_recursively_in_json_values() {
        let mut value = serde_json::json!({
            "details": ["Authorization: Bearer abcdefghijklmnopqrstuvwxyz", {"nested": "s3cr3t-value-123"}]
        });
        redact_json_value_with_known_secrets(
            &mut value,
            &[("DEMO_TOKEN".to_string(), "s3cr3t-value-123".to_string())],
        );
        assert_eq!(
            value["details"][0],
            "Authorization: Bearer [REDACTED:TOKEN]"
        );
        assert_eq!(value["details"][1]["nested"], "[REDACTED:DEMO_TOKEN]");
    }
}
