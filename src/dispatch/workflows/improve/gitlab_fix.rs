//! Carries an unresolved-discussion snapshot across the existing validated repair/push workflow.
use crate::config::Profile;
use crate::dispatch::DispatchArgs;
use crate::ledger::LedgerEntry;
use crate::provider::gitlab_review;
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) struct GitLabFix {
    iid: String,
    threads: Vec<Value>,
    report: PathBuf,
    base: String,
}

impl GitLabFix {
    /// Load review context for either --mr or --existing-branch repairs without changing dispatch trust.
    pub(super) fn prepare(
        profile: &Profile,
        args: &DispatchArgs,
        branch: &str,
        report_dir: &Path,
        base: Option<&str>,
        prompt: &mut String,
        ledger: &mut LedgerEntry,
    ) -> Option<Self> {
        if args.mode != "fix"
            || profile.provider_kind().ok()? != crate::provider_kind::ProviderKind::Gitlab
        {
            return None;
        }
        let base = base?;
        let result = (|| -> Result<Self> {
            let iid = gitlab_review::mr_for_branch(profile, branch)?;
            let threads = gitlab_review::unresolved(profile, &iid, |note| {
                crate::dispatch::issues::gitlab_note_author_is_trusted(profile, note)
            })?;
            if serde_json::to_vec(&threads)?.len() > 60_000 {
                anyhow::bail!("discussion context too large");
            }
            let report = report_dir
                .canonicalize()?
                .join("gitlab-addressed-threads.json");
            prompt.push_str(&gitlab_review::fix_prompt(&threads, &report, |note| {
                crate::dispatch::issues::gitlab_note_author_is_trusted(profile, note)
            }));
            Ok(Self {
                iid,
                threads,
                report,
                base: base.to_owned(),
            })
        })();
        match result {
            Ok(context) => Some(context),
            Err(_) => {
                record_completion(
                    ledger,
                    Err(anyhow::anyhow!("context unavailable")),
                    "fix context",
                );
                prompt.push_str("\nGitLab discussions could not be loaded. Do not claim to have addressed review threads; human review is required.\n");
                None
            }
        }
    }

    /// A failed validation, denied publication, or failed push never acknowledges a thread.
    pub(super) fn finish(&self, profile: &Profile, ledger: &mut LedgerEntry, wt: &Path) {
        if !ledger.push_succeeded
            || ledger.validation_result.as_deref() != Some("passed")
            || !profile.publishing.allow_issue_comments
            || profile.delivery_mode == crate::config::DeliveryMode::Handoff
        {
            return;
        }
        let result = (|| {
            crate::policy::enforce_policy(profile, "review")?;
            let report = serde_json::from_slice(&std::fs::read(&self.report)?)?;
            let files = crate::worktree::git(&["diff", "--name-only", &self.base, "HEAD"], wt)?
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let sha = crate::worktree::git(&["rev-parse", "HEAD"], wt)?;
            gitlab_review::resolve(
                profile,
                &self.iid,
                &self.threads,
                &report,
                &files,
                sha.trim(),
            )
        })();
        record_completion(ledger, result, "thread acknowledgement");
    }
}

/// Never acknowledge a report left by an earlier failed backend attempt.
pub(super) fn start_attempt(context: &mut Option<GitLabFix>, ledger: &mut LedgerEntry) {
    if context
        .as_ref()
        .is_some_and(|c| std::fs::write(&c.report, "[]").is_err())
    {
        *context = None;
        record_completion(
            ledger,
            Err(anyhow::anyhow!("report unavailable")),
            "fix report",
        );
    }
}

/// Preserve incomplete provider operations on the entry saved by dispatch, without leaking API text.
fn record_completion(ledger: &mut LedgerEntry, result: Result<()>, operation: &str) {
    if !gitlab_review::soft_failure(result, operation) {
        ledger.human_required = true;
        ledger.human_required_reason_code.get_or_insert_with(|| {
            crate::controller::HumanRequiredReason::ConfigurationInfra
                .as_str()
                .into()
        });
        let message = format!("GitLab {operation} incomplete; human review required");
        match &mut ledger.error_summary {
            Some(summary) => {
                summary.push_str("; ");
                summary.push_str(&message);
            }
            None => ledger.error_summary = Some(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlab_retry_clears_prior_acknowledgements_and_disables_resolution_if_report_is_unwritable()
    {
        let temp = tempfile::tempdir().unwrap();
        let report = temp.path().join("report.json");
        std::fs::write(&report, r#"[{"discussion_id":"earlier-attempt"}]"#).unwrap();
        let mut context = Some(GitLabFix {
            iid: "7".into(),
            threads: vec![],
            report: report.clone(),
            base: "base".into(),
        });
        let profile = crate::ledger::test_util::profile();
        let mut ledger = LedgerEntry::new("test", &profile, "auto", "fix", "#7", None, None);
        start_attempt(&mut context, &mut ledger);
        assert!(!ledger.human_required);
        assert_eq!(std::fs::read_to_string(&report).unwrap(), "[]");
        std::fs::remove_file(&report).unwrap();
        std::fs::create_dir(&report).unwrap();
        start_attempt(&mut context, &mut ledger);
        assert!(context.is_none());
        assert!(ledger.human_required);
        assert!(ledger.error_summary.unwrap().contains("fix report"));
    }
}
