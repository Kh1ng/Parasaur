use crate::models::{PolicyConfig, RepoPolicy};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Read a policy file and answer whether it permits one repository mutation.
/// CLI, dispatch, and reconciliation checks share this rule table so they
/// cannot disagree about the same action.
pub(crate) fn config_allows_action(config: &Path, action: &str) -> Result<bool> {
    let text = fs::read_to_string(config)
        .with_context(|| format!("reading policy file: {}", config.display()))?;
    let config: PolicyConfig = toml::from_str(&text)
        .with_context(|| format!("parsing policy file: {}", config.display()))?;
    Ok(repo_allows_action(&config.repo, action))
}

fn repo_allows_action(repo: &RepoPolicy, action: &str) -> bool {
    match repo.trust_mode.as_str() {
        "read_only" => false,
        "draft_pr_allowed" => match action {
            "open-draft-pr" => {
                repo.allow_provider_mutation && repo.allow_push && repo.allow_draft_pr
            }
            "edit-issue" => repo.allow_issue_write,
            "merge" | "review" | "mark-ready" => repo.allow_provider_mutation,
            "git-push" => repo.allow_push,
            "git-push-prod" | "update-agent-cli" => repo.allow_project_write,
            _ => false,
        },
        _ => false,
    }
}

/// Refuse a repository mutation unless the profile's current policy permits it.
pub(crate) fn enforce_policy(profile: &crate::config::Profile, action: &str) -> Result<()> {
    let Some(policy_path) = &profile.policy_path else {
        return Ok(()); // no policy file = trust the user
    };
    let allowed = config_allows_action(Path::new(policy_path), action)
        .map_err(|error| PolicyBlocked(format!("POLICY BLOCKED: {error:#}")))?;
    if allowed {
        Ok(())
    } else {
        Err(PolicyBlocked(format!(
            "POLICY BLOCKED: action={action:?} is not allowed by {policy_path}. \
             Set the matching permission in that policy or pass --override-policy if you know what you're doing."
        )).into())
    }
}

/// A durable policy refusal, distinct from a retryable provider failure.
#[derive(Debug, Clone)]
pub(crate) struct PolicyBlocked(String);

impl std::fmt::Display for PolicyBlocked {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PolicyBlocked {}

pub(crate) fn policy_blocked(error: &anyhow::Error) -> Option<&PolicyBlocked> {
    error.chain().find_map(|cause| cause.downcast_ref())
}

/// Reuse the ledger's durable human gate so a refusal is not retried each tick.
pub(crate) fn stamp_refusal(entry: &mut crate::ledger::LedgerEntry, error: &anyhow::Error) {
    entry.work_id = entry.work_id.take().or_else(|| entry.branch.clone());
    entry.dispatch_reason = Some("repo_policy_blocked".into());
    entry.human_required = true;
    entry.human_required_reason_code = Some("policy_approval".into());
    entry.set_failure(
        crate::ledger::FailureClass::HumanBlocked,
        crate::ledger::FailureStage::Dispatch,
    );
    entry.validation_result = Some("policy_blocked".into());
    entry.review_contract_version = Some(crate::ledger::REVIEW_CONTRACT_VERSION);
    entry.error_summary = Some(format!("{error:#}"));
}

pub fn run(config: &str, action: &str) -> Result<()> {
    if config_allows_action(Path::new(config), action)? {
        println!("allowed");
        Ok(())
    } else {
        println!("blocked");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RepoPolicy {
        RepoPolicy {
            trust_mode: "draft_pr_allowed".into(),
            allow_provider_mutation: true,
            allow_push: true,
            allow_draft_pr: true,
            allow_issue_write: false,
            allow_project_write: false,
        }
    }

    #[test]
    fn one_rule_table_covers_every_mutation_action() {
        let policy = policy();
        assert!(repo_allows_action(&policy, "open-draft-pr"));
        assert!(repo_allows_action(&policy, "git-push"));
        assert!(!repo_allows_action(&policy, "edit-issue"));
        assert!(!repo_allows_action(&policy, "git-push-prod"));
        assert!(!repo_allows_action(&policy, "unknown"));
        for action in ["merge", "review", "mark-ready"] {
            assert!(repo_allows_action(&policy, action));
        }
    }

    #[test]
    fn agent_updates_require_project_write_and_known_write_trust() {
        let mut policy = policy();
        for trust in ["read_only", "future_mode", "draft_pr_allowed"] {
            for allowed in [true, false] {
                policy.trust_mode = trust.into();
                policy.allow_project_write = allowed;
                assert_eq!(
                    repo_allows_action(&policy, "update-agent-cli"),
                    trust == "draft_pr_allowed" && allowed
                );
            }
        }
    }

    #[test]
    fn unknown_trust_modes_fail_closed() {
        let mut policy = policy();
        policy.trust_mode = "future_mode".into();
        assert!(!repo_allows_action(&policy, "edit-issue"));
        assert!(!repo_allows_action(&policy, "open-draft-pr"));
    }

    #[test]
    fn provider_actions_require_provider_permission_and_known_write_trust() {
        let mut policy = policy();
        for trust in ["read_only", "future_mode", "draft_pr_allowed"] {
            for allowed in [true, false] {
                policy.trust_mode = trust.into();
                policy.allow_provider_mutation = allowed;
                for action in ["merge", "review", "mark-ready"] {
                    assert_eq!(
                        repo_allows_action(&policy, action),
                        trust == "draft_pr_allowed" && allowed
                    );
                }
            }
        }
    }
}
