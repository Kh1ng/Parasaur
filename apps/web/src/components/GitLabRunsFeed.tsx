import { useEffect, useState } from 'react';
import { Activity, ExternalLink, GitBranch } from 'lucide-react';
import type { GitLabRunSummary, GitLabRunsFeedResponse } from '@git-agent-harness/contracts';
import { gahApi } from '../api/client.js';
import { useAutoRefresh } from '../hooks/useAutoRefresh.js';
import { ExternalAnchor } from './ExternalAnchor.js';
import { EmptyState, ErrorState, LoadingState } from './ui/EmptyState.js';
import { StatusBadge, type StatusTone } from './ui/StatusBadge.js';
import { formatLocalTime } from '../lib/format.js';

/** The only embed page kinds that have runs (#1595). */
const RUN_KINDS = ['issue', 'merge_request'] as const;
/** The panel polls the feed at the dashboard's cadence. */
const RUNS_REFRESH_MS = 30 * 1000;

function runKindOf(kind: string | null): (typeof RUN_KINDS)[number] | null {
  return RUN_KINDS.find(value => value === kind) ?? null;
}

export type GitLabRunsFeedProps = {
  /** The GitLab page context: origin plus the issue/MR identity. `kind`
   * accepts every embed kind but only issue and merge_request have runs. */
  context: {
    gitlab: string;
    project: string;
    kind: string | null;
    iid: number | null;
    ref?: string | null;
  };
  /** Base URL for run-detail deep links (the dashboard or its /parasaur
   * proxy). Defaults to the current deployment's base. */
  panelUrl?: string;
  /** In-app navigation for the standalone page; the embed view gets an
   * external deep link instead. */
  onOpenRun?: (run: GitLabRunSummary) => void;
  limit?: number;
};

const STATUS_TONES: Record<string, StatusTone> = {
  running: 'good',
  starting: 'warning',
  stopping: 'warning',
  passed: 'good',
  failed: 'critical',
  held: 'warning',
  reviewed: 'serious'
};

function runTone(status: string): StatusTone {
  return STATUS_TONES[status] ?? 'unknown';
}

/** The dashboard deep link for one run. Live sessions open the session
 * detail modal (#1643's `?session=` handling); ledger runs land on the
 * profile's work page where the work item's attempts live. */
export function runDetailUrl(panelUrl: string, run: GitLabRunSummary): string {
  const url = new URL(panelUrl);
  url.searchParams.set('page', 'work');
  if (run.profile) url.searchParams.set('profile', run.profile);
  if (run.source === 'session') url.searchParams.set('session', run.id);
  return url.href;
}

function defaultPanelUrl(): string {
  return new URL('.', window.location.href).href;
}

function RunRow({ run, detailHref, onOpenRun }: { run: GitLabRunSummary; detailHref: string | null; onOpenRun?: (run: GitLabRunSummary) => void }) {
  const started = formatLocalTime(run.startedAt);
  return (
    <li className="flex flex-wrap items-center justify-between gap-2 px-3 py-2.5">
      <div className="min-w-0">
        <p className="flex flex-wrap items-center gap-2 text-sm text-primary">
          <StatusBadge tone={runTone(run.status)} label={run.status} />
          <span className="font-medium">{run.mode ?? 'run'}</span>
          {run.workId && <span className="font-mono text-xs text-muted">{run.workId}</span>}
        </p>
        <p className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-muted">
          {started && <span>Started {started}</span>}
          {run.branch && (
            <span className="inline-flex items-center gap-1">
              <GitBranch size={12} aria-hidden="true" />
              {run.branch}
            </span>
          )}
          {run.backend && <span className="inline-flex items-center gap-1">{run.backend}{run.model ? ` · ${run.model}` : ''}</span>}
          {run.testStatus && <span data-testid="run-test-status">Tests: {run.testStatus}</span>}
          {run.durationSeconds !== null && <span>{Math.round(run.durationSeconds)}s</span>}
          {run.attempts.length > 1 && <span>{run.attempts.length} attempts</span>}
        </p>
      </div>
      <div className="flex items-center gap-2 text-xs">
        {run.mrUrl && (
          <ExternalAnchor href={run.mrUrl} className="text-accent underline-offset-4 hover:underline">
            MR
          </ExternalAnchor>
        )}
        {onOpenRun ? (
          <button type="button" onClick={() => onOpenRun(run)} className="text-accent underline-offset-4 hover:underline">
            Details
          </button>
        ) : (
          detailHref && (
            <ExternalAnchor href={detailHref} className="inline-flex items-center gap-1 text-accent underline-offset-4 hover:underline">
              Details
              <ExternalLink size={11} aria-hidden="true" />
            </ExternalAnchor>
          )
        )}
      </div>
    </li>
  );
}

/**
 * Runs for the GitLab page the panel is looking at (#1595): live dispatch
 * sessions and durable ledger runs for this issue or merge request, newest
 * first, each with backend, model, test status and the resulting MR link.
 * Mountable from the embed view (/embed) and the standalone dashboard:
 * every data access goes through `fetchRuns`, and `onOpenRun` vs the
 * external deep link decides how a run detail opens.
 */
export function GitLabRunsFeed({ context, panelUrl, onOpenRun, limit }: GitLabRunsFeedProps) {
  const [feed, setFeed] = useState<GitLabRunsFeedResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  const kind = runKindOf(context.kind);
  const scoped = kind !== null && context.iid !== null ? { kind, iid: context.iid } : null;
  const refresh = () => {
    if (!scoped) return;
    gahApi.getGitLabRuns({
      project: context.project,
      kind: scoped.kind,
      iid: scoped.iid,
      gitlab: context.gitlab,
      branch: context.ref ?? undefined,
      limit
    })
      .then(result => { setFeed(result); setError(null); })
      .catch(reason => { setError(reason instanceof Error ? reason.message : String(reason)); });
  };

  useEffect(refresh, [context.project, scoped?.kind, scoped?.iid, context.gitlab, context.ref, limit]);
  useAutoRefresh(refresh, RUNS_REFRESH_MS);

  if (!scoped) return null;
  if (error) return <ErrorState message={error} endpoint="/api/integrations/gitlab/runs" onRetry={refresh} />;
  if (!feed) return <LoadingState label="Loading runs…" />;
  const detailBase = panelUrl ?? defaultPanelUrl();
  return (
    <section aria-label="Parasaur runs" className="card">
      <ul className="divide-y divide-subtle" data-testid="gitlab-runs-list">
        {feed.runs.length === 0 ? (
          <li>
            <EmptyState
              icon={Activity}
              title="No runs for this context yet"
              description={feed.context.profiles.length > 0
                ? 'Runs Parasaur starts for this issue or merge request appear here.'
                : 'No Parasaur profile is configured for this project.'}
            />
          </li>
        ) : (
          feed.runs.map(run => (
            <RunRow key={run.id} run={run} detailHref={onOpenRun ? null : runDetailUrl(detailBase, run)} onOpenRun={onOpenRun} />
          ))
        )}
      </ul>
    </section>
  );
}
