/**
 * Context-scoped runs feed for the GitLab panel (#1595).
 *
 * Pure filtering over data the server already exposes: live fleet sessions,
 * the dispatch ledger (read through the same `gah report` ledger path as
 * /api/report/roles), and controller events (`gah events`). A GitLab
 * context is provider host (optional), project path, kind (issue |
 * merge_request) and iid; runs whose session carried the structured
 * GitLabDispatchContext (#1592's dispatcher records it) match directly,
 * older or foreign runs match by the identities the ledger already keeps
 * (repo, source_issue_number, `mr:` work ids, MR URLs, target text).
 */
import type {
  ControllerEvent,
  GitLabContextKind,
  GitLabDispatchContext,
  GitLabFeedEvent,
  GitLabRunAttempt,
  GitLabRunSummary,
  GitLabRunsFeedResponse,
  LedgerEntry,
  ProfileSummary,
  Session
} from '@git-agent-harness/contracts';
import { readLedger } from '../../roleMetrics.js';
import { runEvents, runProfileList, runReport } from '../../gahCli.js';
import { getFleetDispatch } from '../../wsServer.js';

/** Ledger modes that bookkeep policy, not a model doing a job. */
const NOT_RUNS = new Set(['review_hold', 'review_hold_release', 'clear_attempts', 'tombstone']);

export const GITLAB_RUNS_FEED_DEFAULT_LIMIT = 20;
export const GITLAB_RUNS_FEED_MAX_LIMIT = 100;
/** Controller event details are trimmed to this before leaving the server. */
const FEED_EVENT_MESSAGE_MAX = 200;

export type GitLabRunsContext = {
  project: string;
  kind: GitLabContextKind;
  iid: number;
  /** GitLab origin, when the caller can prove it; narrows matches. */
  origin: string | null;
  branch: string | null;
  limit: number;
};

export type GitLabRunsQueryParseFailure = { error: string };

function positiveInt(value: string | undefined): number | null {
  if (value === undefined || !/^[1-9][0-9]*$/.test(value)) return null;
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) ? parsed : null;
}

/** Origin of a web URL, or null when it is missing/unparseable. */
function originOf(url: string | null | undefined): string | null {
  if (!url) return null;
  try {
    return new URL(url).origin;
  } catch {
    return null;
  }
}

/**
 * Parse and validate `GET /api/integrations/gitlab/runs` query parameters.
 * Every failure is a 400: an empty or malformed context must not quietly
 * widen the feed to every run.
 */
export function parseGitLabRunsQuery(params: Record<string, unknown>): GitLabRunsContext | GitLabRunsQueryParseFailure {
  const project = typeof params.project === 'string' ? params.project.trim() : '';
  if (!project || project.length > 512 || /[\x00-\x1f\x7f]/.test(project)) return { error: 'project is required: the full GitLab project path' };
  const kind = params.kind;
  if (kind !== 'issue' && kind !== 'merge_request') return { error: 'kind must be issue or merge_request' };
  const iid = positiveInt(typeof params.iid === 'string' ? params.iid : undefined);
  if (iid === null) return { error: 'iid is required: a positive issue or merge request iid' };
  let origin: string | null = null;
  if (params.gitlab !== undefined) {
    if (typeof params.gitlab !== 'string' || !params.gitlab.trim()) return { error: 'gitlab must be the GitLab origin URL' };
    origin = originOf(params.gitlab.trim());
    if (origin === null) return { error: 'gitlab must be the GitLab origin URL' };
  }
  let branch: string | null = null;
  if (params.branch !== undefined) {
    if (typeof params.branch !== 'string' || !params.branch.trim() || params.branch.length > 512 || /[\x00-\x1f\x7f]/.test(params.branch)) return { error: 'branch must be a branch name' };
    branch = params.branch.trim();
  }
  let limit = GITLAB_RUNS_FEED_DEFAULT_LIMIT;
  if (params.limit !== undefined) {
    const parsed = positiveInt(typeof params.limit === 'string' ? params.limit : undefined);
    if (parsed === null || parsed > GITLAB_RUNS_FEED_MAX_LIMIT) return { error: `limit must be an integer between 1 and ${GITLAB_RUNS_FEED_MAX_LIMIT}` };
    limit = parsed;
  }
  return { project, kind, iid, origin, branch, limit };
}

/** MR iid from a GitLab merge request URL, or null for anything else. */
export function mrIidFromUrl(url: string | null | undefined): number | null {
  if (!url) return null;
  try {
    const segments = new URL(url).pathname.split('/').filter(Boolean);
    const at = segments.lastIndexOf('merge_requests');
    if (at < 0 || at + 1 >= segments.length) return null;
    return positiveInt(segments[at + 1]);
  } catch {
    return null;
  }
}

/** Issue/MR URL reference inside instruction text, e.g. the GitLab note
 * dispatcher's target: `…/-/issues/42:`. */
function targetReferences(path: 'issues' | 'merge_requests', iid: number, target: string | null | undefined): boolean {
  return target !== null && target !== undefined && new RegExp(`/-/${path}/${iid}(?![0-9])`).test(target);
}

/** The project+origin a structured context proves, or null when it cannot
 * prove this query's project (or origin, when one was demanded). */
function structuredContextProves(gitlab: GitLabDispatchContext | undefined, context: GitLabRunsContext): boolean {
  if (!gitlab || gitlab.project !== context.project || gitlab.kind !== context.kind || gitlab.iid !== context.iid) return false;
  return context.origin === null || originOf(gitlab.webUrl) === context.origin;
}

/** Whether a live dispatch session belongs to the GitLab context. */
export function sessionMatchesGitLabContext(session: Session, context: GitLabRunsContext): boolean {
  if (session.gitlab) return structuredContextProves(session.gitlab, context);
  // Derivation for sessions started before the dispatcher recorded the
  // context (#1592 adoption): the GitLab dispatcher starts gitlab sessions
  // with repo = project path, target `#<iid>` (assigned issues) or
  // instruction text embedding the resource URL, and mr = `<iid>`.
  if (session.providerKind !== 'gitlab' || session.repo !== context.project) return false;
  if (context.origin !== null) return false; // An unstructured session cannot prove its host.
  if (context.branch !== null && session.branch !== context.branch) return false;
  if (context.kind === 'merge_request') {
    return session.mr === String(context.iid) || targetReferences('merge_requests', context.iid, session.target);
  }
  const target = session.target?.trim();
  return target === `#${context.iid}` || target === String(context.iid) || targetReferences('issues', context.iid, session.target);
}

/** Whether a durable ledger entry belongs to the GitLab context. */
export function ledgerEntryMatchesGitLabContext(entry: LedgerEntry, context: GitLabRunsContext): boolean {
  if (entry.provider !== 'gitlab' || entry.repo !== context.project) return false;
  if (context.branch !== null && entry.branch !== context.branch) return false;
  if (NOT_RUNS.has(entry.mode)) return false;
  if (context.kind === 'merge_request') {
    return entry.work_id === `mr:${context.iid}`
      || mrIidFromUrl(entry.mr_url) === context.iid
      || targetReferences('merge_requests', context.iid, entry.target_summary);
  }
  return entry.source_issue_number === String(context.iid)
    || entry.work_id === `#${context.iid}`
    || targetReferences('issues', context.iid, entry.target_summary);
}

/** Coarse run status from a ledger record's outcome fields. */
function ledgerRunStatus(entry: LedgerEntry): string {
  if (entry.error_summary) return 'failed';
  const validation = entry.validation_result;
  if (validation === 'passed') return 'passed';
  if (typeof validation === 'string' && validation.startsWith('failed')) return 'failed';
  if (entry.human_required) return 'held';
  if (entry.review_verdict) return 'reviewed';
  return 'finished';
}

function attemptsOf(entry: LedgerEntry): GitLabRunAttempt[] {
  return (entry.attempts ?? []).map((attempt) => ({
    attempt: attempt.attempt_number,
    backend: attempt.backend,
    model: attempt.effective_model,
    exitCode: attempt.exit_code,
    validationResult: attempt.validation_result,
    failureClass: attempt.failure_class,
    durationSeconds: attempt.duration_seconds
  }));
}

/** Ledger entry → feed run. The id is stable per entry (profile, timestamp,
 * mode), unique enough for list keys and deep links to the work timeline. */
export function gitLabRunFromLedgerEntry(entry: LedgerEntry): GitLabRunSummary {
  return {
    id: `ledger:${entry.profile}:${entry.timestamp}:${entry.mode}`,
    source: 'ledger',
    status: ledgerRunStatus(entry),
    mode: entry.mode,
    workId: entry.work_id ?? null,
    branch: entry.branch,
    backend: entry.effective_backend || entry.requested_backend || null,
    model: entry.effective_model ?? entry.requested_model ?? null,
    startedAt: entry.timestamp,
    endedAt: entry.timestamp,
    durationSeconds: entry.duration_seconds,
    testStatus: entry.validation_result ?? null,
    mrUrl: entry.mr_url,
    title: entry.work_title ?? entry.target_summary ?? null,
    profile: entry.profile,
    attempts: attemptsOf(entry)
  };
}

/** Live session → feed run. Only in-flight sessions are projected; terminal
 * ones are the ledger's job, so a finished run is never listed twice. */
export function gitLabRunFromSession(session: Session, profile: string | null): GitLabRunSummary {
  // An instruction-text target is not a work id; only ticket-like tokens count.
  const targetToken = session.target?.trim() ?? '';
  const workId = /^#[1-9][0-9]*$/.test(targetToken) ? targetToken : session.mr ? `mr:${session.mr}` : null;
  return {
    id: session.id,
    source: 'session',
    status: session.status,
    mode: session.mode ?? null,
    workId,
    branch: session.branch ?? null,
    backend: session.backend ?? null,
    model: session.model ?? null,
    startedAt: session.startedAt ?? new Date().toISOString(),
    endedAt: session.endedAt ?? null,
    durationSeconds: null,
    testStatus: null,
    mrUrl: null,
    title: null,
    profile,
    attempts: []
  };
}

/** Controller event → slim feed event; details are truncated. */
export function gitLabFeedEvent(event: ControllerEvent): GitLabFeedEvent {
  return {
    timestamp: event.timestamp,
    eventType: event.event_type,
    workId: event.work_id ?? null,
    message: event.details.length > FEED_EVENT_MESSAGE_MAX ? `${event.details.slice(0, FEED_EVENT_MESSAGE_MAX)}…` : event.details
  };
}

/** Configured GitLab profiles for the project, narrowed by origin when given. */
export function matchingGitLabProfiles(profiles: ProfileSummary[], context: GitLabRunsContext): ProfileSummary[] {
  return profiles.filter(profile => profile.provider === 'gitlab' && profile.repo === context.project
    && (context.origin === null || originOf(profile.web_url) === context.origin));
}

export type GitLabRunsFeedInputs = {
  profiles: ProfileSummary[];
  /** All live fleet sessions. */
  sessions: Session[];
  /** The full ledger, as the /api/report/roles reader loads it. */
  ledgerEntries: LedgerEntry[];
  /** Controller events per profile name. */
  events: Record<string, ControllerEvent[]>;
};

/** Assemble the feed: newest first, bounded by the query's limit. */
export function buildGitLabRunsFeed(inputs: GitLabRunsFeedInputs, context: GitLabRunsContext): GitLabRunsFeedResponse {
  const matchedProfiles = matchingGitLabProfiles(inputs.profiles, context);
  const matchedProfileNames = new Set(matchedProfiles.map(profile => profile.name));
  const profileForRuns = matchedProfiles.length === 1 ? matchedProfiles[0].name : null;

  const liveRuns = inputs.sessions
    .filter(session => ['starting', 'running', 'stopping'].includes(session.status))
    .filter(session => sessionMatchesGitLabContext(session, context))
    .map(session => gitLabRunFromSession(session, profileForRuns));

  const ledgerEntries = inputs.ledgerEntries
    .filter(entry => ledgerEntryMatchesGitLabContext(entry, context))
    // When an origin was demanded, only entries from profiles it provably
    // belongs to may answer; unconfigured profiles cannot prove a host.
    .filter(entry => context.origin === null || matchedProfileNames.has(entry.profile));

  const workIds = new Set<string>([
    ...ledgerEntries.flatMap(entry => entry.work_id ? [entry.work_id] : []),
    ...(context.kind === 'issue' ? [`#${context.iid}`] : [`mr:${context.iid}`])
  ]);
  const events = Object.entries(inputs.events)
    .filter(([profile]) => matchedProfileNames.has(profile))
    .flatMap(([, events]) => events)
    .filter(event => event.work_id !== null && workIds.has(event.work_id))
    .map(gitLabFeedEvent);

  const runs = [...liveRuns, ...ledgerEntries.map(gitLabRunFromLedgerEntry)]
    .sort((left, right) => Date.parse(right.startedAt) - Date.parse(left.startedAt) || left.id.localeCompare(right.id))
    .slice(0, context.limit);

  return {
    context: {
      project: context.project,
      kind: context.kind,
      iid: context.iid,
      webUrl: matchedProfiles.find(profile => profile.web_url)?.web_url ?? null,
      profiles: matchedProfiles.map(profile => profile.name)
    },
    runs,
    events: events
      .sort((left, right) => Date.parse(right.timestamp) - Date.parse(left.timestamp))
      .slice(0, context.limit)
  };
}

export type GitLabRunsFeedDeps = {
  listProfiles: typeof runProfileList;
  /** Ledger path via the same `gah report` indirection as /api/report/roles. */
  report: typeof runReport;
  ledger: (path: string) => LedgerEntry[];
  events: typeof runEvents;
  sessions: () => Session[];
};

const defaultDeps: GitLabRunsFeedDeps = {
  listProfiles: runProfileList,
  report: runReport,
  ledger: readLedger,
  events: runEvents,
  // The fleet coordinator is assigned when the WebSocket handler boots; a
  // read that races it still answers with the ledger's durable history.
  sessions: () => {
    const fleet = getFleetDispatch() as ReturnType<typeof getFleetDispatch> | undefined;
    return fleet ? fleet.getAllSessions() : [];
  }
};

/** Load every input and build the feed for a validated context. */
export async function loadGitLabRunsFeed(context: GitLabRunsContext, deps: GitLabRunsFeedDeps = defaultDeps): Promise<GitLabRunsFeedResponse> {
  const profiles = await deps.listProfiles();
  const matched = matchingGitLabProfiles(profiles, context);
  const [sessions, report, ...eventLists] = await Promise.all([
    deps.sessions(),
    deps.report({ since: '1d' }),
    ...matched.map(profile => deps.events(profile.name, '7d'))
  ]);
  const events: Record<string, ControllerEvent[]> = {};
  matched.forEach((profile, index) => { events[profile.name] = eventLists[index] ?? []; });
  return buildGitLabRunsFeed({ profiles, sessions, ledgerEntries: deps.ledger(report.ledger_path), events }, context);
}
