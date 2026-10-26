import type { EventPolicySummary } from '@git-agent-harness/contracts';
import { runConfigShowProfile, runProfileList } from '../../gahCli.js';
import { execProviderCli } from '../../managerChat/providerCli.js';
import { assertIssueNotManaged } from '../../managedIssues.js';
import type { SessionOptions } from '../../sessions/SessionManager.js';
import { getFleetDispatch } from '../../wsServer.js';
import { record, type GitLabEvent } from './classifier.js';

export type GitLabDispatchDeps = {
  listProfiles: typeof runProfileList;
  config: typeof runConfigShowProfile;
  cli: typeof execProviderCli;
  start: (options: SessionOptions) => Promise<{ id: string }>;
  managed: typeof assertIssueNotManaged;
};

/** Use the same fleet coordinator as POST /api/dispatch and GitHub dashboard starts. */
const defaults: GitLabDispatchDeps = {
  listProfiles: runProfileList,
  config: runConfigShowProfile,
  cli: execProviderCli,
  start: options => getFleetDispatch().startSession(options),
  managed: assertIssueNotManaged,
};

function trusted(author: Record<string, unknown>, policy: EventPolicySummary, projectId: number): boolean {
  if (typeof author.username !== 'string' || !author.username.trim()) return false;
  const login = author.username.trim();
  const bot = author.bot === true || author.is_bot === true
    || (typeof author.bot !== 'boolean' && typeof author.is_bot !== 'boolean' && login.startsWith(`project_${projectId}_bot_`));
  const allowed = bot ? policy.trusted_issue_bot_authors : policy.trusted_issue_human_authors;
  return Array.isArray(allowed) && allowed.some(name => name.toLowerCase() === login.toLowerCase());
}

/** Resolve configured project and authoritative author policy before starting any worker. */
export async function dispatchGitLabEvent(
  event: GitLabEvent,
  options: { bot: string; panelUrl: string },
  deps: GitLabDispatchDeps = defaults
): Promise<void> {
  const profiles = (await deps.listProfiles()).filter(profile => profile.provider === 'gitlab'
    && profile.repo === event.project.repo && profile.web_url?.replace(/\/$/, '') === event.project.url);
  if (profiles.length !== 1) return;
  const profile = profiles[0];
  const policy = (await deps.config(profile.name)).event_policy;
  if (!policy) return; // Older CLIs cannot prove author trust.
  const api = async (path: string, fields: string[] = []) => {
    const { stdout } = await deps.cli('glab', ['api', path, '--hostname', new URL(profile.web_url!).host,
      '--method', fields.length ? 'POST' : 'GET', ...fields.flatMap(field => ['--raw-field', field])], profile.local_path);
    return JSON.parse(stdout || 'null') as unknown;
  };
  const project = `projects/${encodeURIComponent(profile.repo)}`;
  const identity = record(await api(project));
  if (identity.id !== event.project.id) return;
  const actor = record(await api(`users/${event.actorId}`));
  if ((event.kind === 'note' && typeof actor.username === 'string' && actor.username.toLowerCase() === options.bot.toLowerCase()) || !trusted(actor, policy, event.project.id)) return;

  let dispatch: Pick<SessionOptions, 'mode' | 'target' | 'mr'>;
  let comment: { resource: 'issues' | 'merge_requests'; iid: number };
  switch (event.kind) {
    case 'issue': {
      const issue = record(await api(`${project}/issues/${event.iid}`));
      if (issue.state !== 'opened' || !trusted(record(issue.author), policy, event.project.id)) return;
      dispatch = { mode: 'improve', target: `#${event.iid}` };
      comment = { resource: 'issues', iid: event.iid };
      break;
    }
    case 'merge_request':
      dispatch = { mode: 'review', mr: String(event.iid) };
      comment = { resource: 'merge_requests', iid: event.iid };
      break;
    case 'note':
      dispatch = {
        mode: event.resource === 'merge_requests' ? 'fix' : 'improve',
        target: `GitLab instruction for ${event.project.url}/-/${event.resource}/${event.iid}:\n\n${event.instruction}`,
        ...(event.resource === 'merge_requests' ? { mr: String(event.iid) } : {}),
      };
      comment = { resource: event.resource, iid: event.iid };
      // Preserve the issue's managed fence even though the note is the worker's target text.
      if (event.resource === 'issues') await deps.managed({ profile: profile.name, mode: dispatch.mode, target: `#${event.iid}` });
      break;
    case 'pipeline': {
      if (!event.branch.startsWith(`gah/${profile.repo_id}-`)) return;
      const rows = await api(`${project}/merge_requests?state=opened&source_branch=${encodeURIComponent(event.branch)}`);
      const mr = Array.isArray(rows) ? rows.map(record).find(mr => record(mr.author).username === options.bot
        && mr.source_project_id === event.project.id && mr.sha === event.sha && Number.isSafeInteger(mr.iid) && Number(mr.iid) > 0) : undefined;
      if (!mr) return;
      dispatch = { mode: 'fix', mr: String(mr.iid), target: `Repair failed GitLab pipeline ${event.pipelineId} on ${event.branch}. Inspect the pipeline jobs and fix the failure.` };
      comment = { resource: 'merge_requests', iid: Number(mr.iid) };
      break;
    }
    default: {
      const exhaustive: never = event;
      return exhaustive;
    }
  }
  await deps.managed({ profile: profile.name, ...dispatch });
  const session = await deps.start({
    profile: profile.name, repo: profile.repo, providerKind: 'gitlab', instanceId: 'gitlab-0',
    requestId: `gitlab-${event.key}`, ...dispatch,
  });
  if (!policy.allow_issue_comments || profile.delivery_mode === 'handoff') return;
  const url = new URL(options.panelUrl);
  url.searchParams.set('page', 'work');
  url.searchParams.set('profile', profile.name);
  url.searchParams.set('session', session.id);
  try {
    await api(`${project}/${comment.resource}/${comment.iid}/notes`, [`body=Parasaur queued a run: [open run](${url.href}).`]);
  } catch {
    // The run exists. A failed comment must never trigger a second dispatch.
    console.error('[gitlab-events] Queued-run comment failed.');
  }
}
