//! Delivery of resolved bank content, bounded independently of the task prompt.
// ponytail: Hermes/OpenHands also retain native skill IDs; add native inventory
// checks if duplicate skill content becomes a problem.

use std::borrow::Cow;

use crate::runner::backend_runner::{BackendRunner, SkillDelivery};
use crate::skill_bindings::ResolvedSkill;

/// Maximum added bytes, including framing and the truncation notice.
const MAX_INJECTED_BYTES: usize = 32 * 1024;
// Mirror apps/server/src/managerChat/ManagerChatManager.ts::applyBoundSkills:
// heading, trust sentence, versioned sections, trimming, and current-request
// framing must stay identical. The Rust dispatch path additionally caps bytes.
const HEADER: &str =
    "# Bound skills\nThese are trusted instructions from the central GAH skill bank.\n\n";
const REQUEST: &str = "\n\n# Current request\n";
const TRUNCATED: &str = "\n\n[Bound skills truncated to fit the dispatch prompt limit.]";

/// Deliver content only to runners that honor prompt injection. Preserve the
/// memory trust envelope and the complete task when skills exceed the budget.
pub(super) fn prompt<'a>(
    runner: &dyn BackendRunner,
    task: &'a str,
    skills: &[ResolvedSkill],
) -> Cow<'a, str> {
    if runner.skill_delivery() != SkillDelivery::Prompt || skills.is_empty() {
        return Cow::Borrowed(task);
    }
    let budget = MAX_INJECTED_BYTES - HEADER.len() - REQUEST.len() - TRUNCATED.len();
    let mut instructions = String::new();
    let mut truncated = false;
    'skills: for (index, skill) in skills.iter().enumerate() {
        for part in [
            if index == 0 { "" } else { "\n\n" },
            "## ",
            &skill.id,
            "@",
            &skill.version,
            "\n",
            skill.content.trim(),
        ] {
            let prefix = super::utf8_safe_prefix(part, budget - instructions.len());
            instructions.push_str(prefix);
            if prefix.len() < part.len() {
                truncated = true;
                break 'skills;
            }
        }
    }
    if truncated {
        instructions.push_str(TRUNCATED);
        eprintln!(
            "gah: bound skills truncated for {} at {MAX_INJECTED_BYTES} injected bytes",
            runner.kind().as_str()
        );
    }
    let marker = "\nCurrentUserRequest: ";
    let request_start = if task.contains("\nRecalledMemoryUntrusted: ") {
        task.find(marker).map(|index| index + marker.len())
    } else {
        None
    }
    .unwrap_or(0);
    Cow::Owned(format!(
        "{}{HEADER}{instructions}{REQUEST}{}",
        &task[..request_start],
        &task[request_start..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend_kind::BackendKind;
    use crate::runner::backend_runner::for_kind;

    fn skill(content: &str) -> ResolvedSkill {
        ResolvedSkill {
            id: "review".into(),
            version: "2.1.0".into(),
            content: content.into(),
            backends: vec![],
        }
    }

    #[test]
    fn skill_prompt_mirrors_chat_format_and_preserves_memory_envelope() {
        let runner = for_kind(BackendKind::Codex);
        let expected = "# Bound skills\nThese are trusted instructions from the central GAH skill bank.\n\n## review@2.1.0\nCheck the diff.\n\n# Current request\nShip it.";
        assert_eq!(
            prompt(runner.as_ref(), "Ship it.", &[skill(" Check the diff.\n")]),
            expected
        );
        let envelope = "trust preamble\nRecalledMemoryUntrusted: \"fact\"\nCurrentUserRequest: ";
        assert_eq!(
            prompt(
                runner.as_ref(),
                &format!("{envelope}Ship it."),
                &[skill("Check the diff.")]
            ),
            format!("{envelope}{expected}")
        );
    }

    #[test]
    fn skill_prompt_caps_total_bytes_across_skills_and_keeps_the_task() {
        let runner = for_kind(BackendKind::Claude);
        let skills = vec![
            skill("first"),
            skill(&"🦀".repeat(MAX_INJECTED_BYTES)),
            skill("omitted"),
        ];
        let task = "complete current request";
        let result = prompt(runner.as_ref(), task, &skills);
        assert!(result.len() - task.len() <= MAX_INJECTED_BYTES);
        assert!(result.contains("first"));
        assert!(!result.contains("omitted"));
        assert_eq!(result.matches(TRUNCATED).count(), 1);
        assert!(result.ends_with(&format!("{REQUEST}{task}")));
        // Metadata is subject to the same budget as content.
        let mut huge_id = skill("unreachable");
        huge_id.id = "id".repeat(MAX_INJECTED_BYTES);
        assert!(prompt(runner.as_ref(), task, &[huge_id]).len() - task.len() <= MAX_INJECTED_BYTES);
    }

    #[test]
    fn empty_skill_resolution_preserves_the_prompt() {
        let runner = for_kind(BackendKind::Opencode);
        assert!(matches!(
            prompt(runner.as_ref(), "task", &[]),
            Cow::Borrowed("task")
        ));
    }

    #[test]
    fn unsupported_runners_preserve_the_prompt() {
        for kind in [BackendKind::Agy, BackendKind::Vibe] {
            let runner = for_kind(kind);
            assert!(matches!(
                prompt(runner.as_ref(), "task", &[skill("instructions")]),
                Cow::Borrowed("task")
            ));
        }
    }
}

/// Review attempts resolve the same bank snapshot as implementation attempts.
#[allow(clippy::too_many_arguments)]
pub(super) fn review_prompt<'a>(
    cfg: &crate::config::GahConfig,
    profile_name: &str,
    profile: &crate::config::Profile,
    identity: &crate::execution_identity::ExecutionIdentity,
    ledger: &mut crate::ledger::LedgerEntry,
    session_dir: &std::path::Path,
    task: &'a str,
) -> anyhow::Result<Cow<'a, str>> {
    let kind = crate::backend_kind::BackendKind::parse(&identity.runner_kind)?;
    let skills = crate::skill_bindings::resolve_for_dispatch(
        &cfg.defaults,
        profile_name,
        identity,
        kind,
        profile.skill_resolve_failure_policy,
        session_dir,
    )?;
    ledger.record_skills(&skills);
    Ok(prompt(
        crate::runner::for_kind(kind).as_ref(),
        task,
        &skills,
    ))
}

#[cfg(test)]
mod version_tests;
