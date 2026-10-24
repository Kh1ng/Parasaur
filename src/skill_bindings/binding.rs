//! Backward-compatible bindings: a label follows newest, an object can pin.
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Binding {
    Label(String),
    Versioned {
        label: String,
        version: Option<String>,
    },
}

impl Binding {
    pub(super) fn label(&self) -> &str {
        match self {
            Self::Label(label) | Self::Versioned { label, .. } => label,
        }
    }

    pub(super) fn version(&self) -> Option<&str> {
        match self {
            Self::Label(_) => None,
            Self::Versioned { version, .. } => version.as_deref(),
        }
    }
}

#[cfg(test)]
impl From<&str> for Binding {
    fn from(label: &str) -> Self {
        Self::Label(label.into())
    }
}

#[test]
fn skill_bindings_reject_empty_resolved_identifiers_before_ledger_capture() {
    for (id, version) in [("", "1"), ("review", "")] {
        let bank: super::SkillBank = serde_json::from_value(serde_json::json!({
            "skills": [{"id": id, "version": version, "content": "instructions"}],
            "bindings": {id: ["backend:codex"]}
        }))
        .unwrap();
        assert!(super::resolve_bank(bank, "test", "codex", None)
            .unwrap_err()
            .to_string()
            .contains("must not be empty"));
    }
}
