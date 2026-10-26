/**
 * GitLab integration contracts (#1591 epic).
 *
 * The companion repo (Kh1ng/parasaur-gitlab) injects a panel into GitLab that
 * iframes Parasaur's embed view with the page's context; the runs feed
 * (#1595) is the read API that view calls to show the runs relevant to the
 * issue or merge request the user is looking at. Dispatches started from
 * GitLab events (#1592's endpoint) carry the same context on the live
 * session, so the feed can filter without re-deriving it from target text.
 */

/** Which GitLab resource a dispatch or feed query is scoped to. */
export type GitLabContextKind = 'issue' | 'merge_request';

/**
 * The GitLab page context a dispatch was started from (#1595): recorded on
 * the live `Session` by the GitLab event dispatcher (#1592) and echoed by the
 * runs feed. Optional everywhere it appears so pre-context sessions and
 * clients stay compatible.
 */
export interface GitLabDispatchContext {
  /** Full project path, e.g. `group/project`. Mirrors `Session.repo`. */
  project: string;
  kind: GitLabContextKind;
  /** Issue iid or MR iid. Iids are per-project, distinct from GitHub's
   * global ids; both are scoped here by `project`. */
  iid: number;
  /** Project web URL (GitLab origin + path), from the triggering event. */
  webUrl: string;
}

/** One run relevant to a GitLab context, newest first. `session` runs are
 * live fleet dispatches; `ledger` runs are durable dispatch records with
 * their per-attempt history. */
export interface GitLabRunSummary {
  /** Live session id (matches `?session=` deep links) or a stable ledger identity. */
  id: string;
  source: 'session' | 'ledger';
  /** Session status for live runs; `running`/`passed`/`failed`/`reviewed` for ledger records. */
  status: string;
  mode: string | null;
  workId: string | null;
  branch: string | null;
  backend: string | null;
  model: string | null;
  /** ISO timestamp of the run's start (session start or ledger entry). */
  startedAt: string;
  endedAt: string | null;
  durationSeconds: number | null;
  /** Test status: the ledger's validation_result, when one ran. */
  testStatus: string | null;
  /** The resulting MR link, when the run produced one. */
  mrUrl: string | null;
  title: string | null;
  /** Profile that owns the run; with `id` this deep-links the run detail.
   * Null only when no configured profile can be attributed. */
  profile: string | null;
  /** Per-attempt history from the ledger (empty for live sessions). */
  attempts: GitLabRunAttempt[];
}

/** One recorded attempt of a ledger run: backend, model, and its outcome. */
export interface GitLabRunAttempt {
  attempt: number;
  backend: string | null;
  model: string | null;
  exitCode: number | null;
  validationResult: string | null;
  failureClass: string | null;
  durationSeconds: number | null;
}

/** Slim controller-event projection for the feed: bounded, no raw payloads. */
export interface GitLabFeedEvent {
  timestamp: string;
  eventType: string;
  workId: string | null;
  /** Truncated controller details; the full payload never leaves the server. */
  message: string;
}

/** GET /api/integrations/gitlab/runs response. */
export interface GitLabRunsFeedResponse {
  context: {
    project: string;
    kind: GitLabContextKind;
    iid: number;
    /** Project web URL, when a configured GitLab profile proves it. */
    webUrl: string | null;
    /** Configured GitLab profiles matching the project (and origin, when given). */
    profiles: string[];
  };
  /** Runs for the context, newest first, at most `limit`. */
  runs: GitLabRunSummary[];
  /** Recent controller events for the same work items, newest first, at most `limit`. */
  events: GitLabFeedEvent[];
}
