use super::{gitlab_api, Profile};
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct GitlabPipeline {
    pub status: Option<String>,
    pub sha: Option<String>,
    id: Option<u64>,
    project_id: Option<u64>,
}

/// GitLab can still list the previous pipeline while creating one for a push.
pub(crate) fn pipeline_for_sha(
    profile: &Profile,
    project_id: &str,
    iid: &str,
    source_sha: &str,
) -> Result<Option<GitlabPipeline>> {
    let endpoint = format!("projects/{project_id}/merge_requests/{iid}/pipelines");
    let response = gitlab_api(profile, &endpoint, "GET", &[("per_page", "100")])?;
    let pipelines: Vec<GitlabPipeline> = serde_json::from_value(response)
        .with_context(|| format!("reading GitLab pipelines for MR !{iid}"))?;
    Ok(pipelines
        .into_iter()
        .find(|pipeline| pipeline.sha.as_deref() == Some(source_sha)))
}

impl GitlabPipeline {
    /// A failed required downstream pipeline can coexist with a broken runner.
    /// Only attribute the failure to the runner when both job lists confirm it.
    pub(crate) fn runner_failure(&self, profile: &Profile, project_id: &str) -> bool {
        if self.status.as_deref() != Some("failed") {
            return false;
        }
        let Some(id) = self.id else { return false };
        let project = self.project_id.map(|id| id.to_string());
        let project = project.as_deref().unwrap_or(project_id);
        let endpoint = format!("projects/{project}/pipelines/{id}");
        let failures = |kind| -> Result<Vec<FailedJob>> {
            let rows = gitlab_api(
                profile,
                &format!("{endpoint}/{kind}"),
                "GET",
                &[("scope[]", "failed"), ("per_page", "100")],
            )?;
            Ok(serde_json::from_value(rows)?)
        };
        // A full page, missing field, or unavailable endpoint cannot establish
        // that every required failure came from the runner. Keep the ordinary
        // failed-pipeline behavior on incomplete evidence.
        failures("jobs").is_ok_and(|jobs| only_runner_failures(&jobs))
            && failures("bridges").is_ok_and(|bridges| {
                bridges.len() < 100 && bridges.iter().all(|bridge| bridge.allow_failure)
            })
    }
}

#[derive(Deserialize)]
struct FailedJob {
    allow_failure: bool,
    status: String,
    failure_reason: Option<String>,
}

fn only_runner_failures(jobs: &[FailedJob]) -> bool {
    let mut required = jobs.iter().filter(|job| !job.allow_failure).peekable();
    jobs.len() < 100
        && required.peek().is_some()
        && required.all(|job| {
            job.status == "failed" && job.failure_reason.as_deref() == Some("runner_system_failure")
        })
}
