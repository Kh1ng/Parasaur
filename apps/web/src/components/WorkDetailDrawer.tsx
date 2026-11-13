import { useEffect, useRef, useState } from 'react';
import { ArrowLeft, ExternalLink, GitPullRequest, Hammer, Pause, Play, RefreshCw, X } from 'lucide-react';
import type { ProviderKind, Session } from '@git-agent-harness/contracts';
import { gahApi, GahApiError } from '../api/client.js';
import { workKey } from '../lib/workKey.js';
import { formatLocalTime } from '../lib/format.js';
import { ciLabelFor, reviewWaitReason } from '../lib/reviewLabels.js';
import { useAutoRefresh } from '../hooks/useAutoRefresh.js';
import { useWsReconnectRefresh } from '../hooks/useWsReconnectRefresh.js';
import { useGahStore, workTimelineKey } from '../store/gahStore.js';
import { AttemptTimeline } from './AttemptTimeline.js';
import { ExternalAnchor } from './ExternalAnchor.js';
import { WaypointHistory, type WaypointEvidence } from './WaypointProgress.js';
import { EmptyState, ErrorState, LoadingState } from './ui/EmptyState.js';
import { LastUpdated } from './ui/LastUpdated.js';
import { StatusBadge, classificationTone } from './ui/StatusBadge.js';
import { CommitPrDialog } from './CommitPrDialog.js';
import { BlockedWorkItems } from './BlockedWorkItems.js';

const REFRESH_MS = 30_000;

type Redispatch = (input: {
  profile: string;
  repo: string;
  workId: string;
  backend: ProviderKind;
}) => void;

type WorkDetailDrawerProps = {
  workId: string;
  profile: string;
  connected: boolean;
  sessions: Session[];
  onClose: () => void;
  onRedispatch: Redispatch;
  /** Inside a sidebar instead of over the page: no dialog, and Close goes back to the list. */
  inline?: boolean;
};

export function WorkDetailDrawer({ workId, profile, connected, sessions, onClose, onRedispatch, inline = false }: WorkDetailDrawerProps) {
  const closeButton = useRef<HTMLButtonElement>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const status = useGahStore((state) => state.status);
  const timeline = useGahStore((state) => state.workTimelines[workTimelineKey(workId, profile)]);
  const profiles = useGahStore((state) => state.profiles);
  const events = useGahStore((state) => state.events);
  const fetchStatus = useGahStore((state) => state.fetchStatus);
  const fetchProfiles = useGahStore((state) => state.fetchProfiles);
  const fetchEvents = useGahStore((state) => state.fetchEvents);
  const fetchWorkTimeline = useGahStore((state) => state.fetchWorkTimeline);
  const [pending, setPending] = useState<'hold' | 'release' | 'clear' | 'dispatch' | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reviewOpen, setReviewOpen] = useState(false);

  const refresh = async () => {
    await Promise.all([
      fetchStatus(profile, { force: true }),
      fetchWorkTimeline(workId, { profile, force: true }),
      fetchEvents({ profile, since: '7d' }, { force: true }),
    ]);
  };

  useEffect(() => {
    void Promise.all([fetchStatus(profile), fetchWorkTimeline(workId, { profile }), fetchProfiles(), fetchEvents({ profile, since: '7d' })]);
    if (!inline) dialog.current?.showModal();
    closeButton.current?.focus();
    return () => dialog.current?.close();
  }, [workId, profile, inline, fetchStatus, fetchProfiles, fetchWorkTimeline, fetchEvents]);

  useAutoRefresh(() => { void refresh(); }, REFRESH_MS);
  useWsReconnectRefresh(() => { void refresh(); });

  const key = workKey(workId);
  const ticket = status.data?.available_tickets.find((item) => workKey(item.work_id ?? item.normalized_work_identity ?? item.ticket_path) === key);
  const mergeRequest = status.data?.merge_requests.find((item) => item.work_id && workKey(item.work_id) === key);
  const blockers = (status.data?.blocked_work_items ?? []).filter((item) => {
    const reference = item.source_reference;
    return (item.remediation_plan?.work_id && workKey(item.remediation_plan.work_id) === key)
      || (reference && (workKey(reference) === key || reference === mergeRequest?.branch || reference === mergeRequest?.url));
  });
  const held = status.data?.review_held_work_ids.some((id) => workKey(id) === key) ?? false;
  const entries = timeline?.data ?? [];
  const workEvents = (events.data ?? [])
    .filter(event => event.profile === profile && event.work_id && workKey(event.work_id) === key)
    .sort((a, b) => Date.parse(b.timestamp) - Date.parse(a.timestamp))
    .slice(0, 10);
  const lastEntry = entries.at(-1);
  const repo = profiles.data?.find((item) => item.name === profile)?.repo ?? lastEntry?.repo ?? null;
  const backend = (ticket?.recommended_backend ?? lastEntry?.effective_backend ?? 'auto') as ProviderKind;
  const title = ticket?.title ?? mergeRequest?.title ?? lastEntry?.work_title ?? workId;
  const session = sessions.find((item) => item.target && workKey(item.target) === key);
  const ledgerEvidence = Object.entries(status.data?.work_waypoint_evidence ?? {})
    .find(([id]) => workKey(id) === key)?.[1];
  const evidence: WaypointEvidence = {
    priorAttemptCount: ticket?.prior_attempt_count,
    hasActiveClaim: ticket?.has_active_claim,
    hasActiveMergeRequest: ticket?.has_active_mr,
    humanRequired: ticket?.human_required,
    sessionStatus: session?.status === 'idle' || session?.status === 'stopping' ? undefined : session?.status,
    sessionStartedAt: session?.startedAt,
    ledgerEvidence,
    mergeRequest
  };

  const run = async (action: typeof pending, mutation: () => Promise<unknown>, success: string) => {
    setPending(action);
    setError(null);
    setResult(null);
    try {
      await mutation();
      await refresh();
      setResult(success);
    } catch (failure) {
      setError(failure instanceof GahApiError ? failure.message : failure instanceof Error ? failure.message : 'Action failed.');
    } finally {
      setPending(null);
    }
  };

  const clearAttempts = () => {
    if (!window.confirm(`Clear prior attempts for ${workId}?`)) return;
    void run('clear', () => gahApi.ledgerClearAttempts({ profile, work_id: workId }), 'Prior attempts cleared.');
  };

  const redispatch = () => {
    if (!repo || !window.confirm(`Re-dispatch ${workId} with ${backend}?`)) return;
    setPending('dispatch');
    setError(null);
    setResult(null);
    // Dispatch is fire-and-forget over the socket; the outcome arrives as
    // activity-feed events, so the drawer reports queuing, not completion.
    onRedispatch({ profile, repo, workId, backend });
    setResult('Dispatch queued. The activity feed reports the outcome.');
    setPending(null);
    void refresh();
  };

  const reviewTone = mergeRequest ? classificationTone(mergeRequest.classification) : { tone: 'unknown' as const, label: 'No review' };
  const ci = ciLabelFor(mergeRequest);
  const reviewWait = reviewWaitReason(mergeRequest);

  const body = (
    <>
        <header className={`flex shrink-0 items-start justify-between gap-4 border-b border-subtle ${inline ? 'pb-3' : 'px-4 py-4 sm:px-6'}`}>
          <div className="min-w-0">
            <p className="font-mono text-xs text-muted">{workId}</p>
            <h2 id="work-detail-title" className="mt-1 break-words text-lg font-semibold text-primary">{title}</h2>
          </div>
          <button ref={closeButton} type="button" onClick={onClose} className="btn-secondary min-h-11 min-w-11 p-2" aria-label={inline ? 'Back to the list' : 'Close work details'}>
            {inline ? <ArrowLeft size={18} aria-hidden="true" /> : <X size={18} aria-hidden="true" />}
          </button>
        </header>

        <div className={`min-w-0 flex-1 overflow-x-hidden ${inline ? 'py-4' : 'overflow-y-auto px-4 py-5 sm:px-6'}`}>
          <section aria-labelledby="work-status-heading" className="card-padded mb-5">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h3 id="work-status-heading" className="text-sm font-semibold text-primary">Current state</h3>
              <LastUpdated at={status.fetchedAt} />
            </div>
            <dl className="mt-3 grid grid-cols-2 gap-3 text-xs sm:grid-cols-4">
              <div><dt className="text-muted">Review</dt><dd className="mt-1"><StatusBadge tone={reviewTone.tone} label={reviewTone.label} /></dd></div>
              <div><dt className="text-muted">Action</dt><dd className="mt-1 break-words text-secondary">{mergeRequest?.recommended_action?.replaceAll('_', ' ') ?? 'None'}</dd></div>
              <div><dt className="text-muted">CI</dt><dd className="mt-1 text-secondary">{ci}</dd></div>
              <div><dt className="text-muted">Hold</dt><dd className="mt-1"><StatusBadge tone={held ? 'warning' : 'good'} label={held ? 'Held' : 'Clear'} /></dd></div>
            </dl>
            <dl className="mt-3 space-y-2 text-xs">
              <div><dt className="text-muted">Reviewer verdict</dt><dd className="mt-1 break-words text-secondary">{mergeRequest?.review_verdict?.replaceAll('_', ' ') ?? 'Not recorded'}</dd></div>
              {mergeRequest?.review_gate_reason && <div><dt className="text-muted">Review gate</dt><dd className="mt-1 break-words text-warning">{mergeRequest.review_gate_reason}</dd></div>}
              {mergeRequest?.review_generation_status && <div><dt className="text-muted">Review evidence</dt><dd className="mt-1 break-words text-warning">{mergeRequest.review_generation_status}</dd></div>}
              {reviewWait && <div><dt className="text-muted">Automatic handoff</dt><dd className="mt-1 break-words text-warning">{reviewWait}</dd></div>}
              {held && <div><dt className="text-muted">Manager hold</dt><dd className="mt-1 text-warning">Automatic merging waits until this hold is cleared.</dd></div>}
            </dl>
            {status.error && <p role="alert" className="mt-3 text-xs text-critical">Factory state could not refresh: {status.error}. Displayed state may be out of date.</p>}
            {mergeRequest?.url && (
              <ExternalAnchor href={mergeRequest.url} className="mt-3 inline-flex min-h-11 items-center gap-1 text-xs text-accent hover:underline sm:min-h-0">
                Open provider review <ExternalLink size={13} aria-hidden="true" />
              </ExternalAnchor>
            )}
          </section>

          {blockers.length > 0 && (
            <section aria-labelledby="work-blockers-heading" className="mb-6">
              <h3 id="work-blockers-heading" className="mb-2 text-sm font-semibold text-primary">What is blocking this work</h3>
              <BlockedWorkItems blockers={blockers} />
            </section>
          )}

          <section aria-labelledby="work-actions-heading" className="mb-6">
            <h3 id="work-actions-heading" className="mb-2 text-sm font-semibold text-primary">Actions</h3>
            <div className="flex flex-wrap gap-2">
              {held ? (
                <button type="button" disabled={pending !== null} onClick={() => void run('release', () => gahApi.holdClear({ profile, work_id: workId }), 'Hold cleared.')} className="btn-secondary min-h-11">
                  <Play size={14} aria-hidden="true" /> {pending === 'release' ? 'Clearing…' : 'Clear hold'}
                </button>
              ) : (
                <button type="button" disabled={pending !== null} onClick={() => void run('hold', () => gahApi.holdSet({ profile, work_id: workId, reason: 'operator review hold from dashboard' }), 'Hold set.')} className="btn-secondary min-h-11">
                  <Pause size={14} aria-hidden="true" /> {pending === 'hold' ? 'Setting…' : 'Set hold'}
                </button>
              )}
              <button type="button" disabled={pending !== null} onClick={clearAttempts} className="btn-secondary min-h-11">
                <Hammer size={14} aria-hidden="true" /> {pending === 'clear' ? 'Clearing…' : 'Clear attempts'}
              </button>
              <button type="button" disabled={pending !== null || !connected || !repo} onClick={redispatch} className="btn-primary min-h-11" title={!repo ? 'Repository details are not available yet.' : undefined}>
                <RefreshCw size={14} aria-hidden="true" /> {pending === 'dispatch' ? 'Dispatching…' : 'Re-dispatch'}
              </button>
              {mergeRequest && (
                <button type="button" disabled={pending !== null} onClick={() => setReviewOpen(true)} className="btn-secondary min-h-11">
                  <GitPullRequest size={14} aria-hidden="true" /> Review in dashboard
                </button>
              )}
            </div>
            {result && <p role="status" className="mt-2 text-xs text-good">{result}</p>}
            {error && <p role="alert" className="mt-2 text-xs text-critical">{error}</p>}
          </section>

          <section aria-labelledby="work-decisions-heading" className="mb-6">
            <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
              <h3 id="work-decisions-heading" className="text-sm font-semibold text-primary">Recent controller decisions</h3>
              <LastUpdated at={events.fetchedAt} />
            </div>
            <p className="mb-3 text-xs text-muted">Up to 10 matching records from the connected controller in the last 7 days.</p>
            {events.loading && !events.data ? <LoadingState label="Loading controller decisions…" />
              : events.error ? <ErrorState message={events.error} endpoint="/api/events" onRetry={() => fetchEvents({ profile, since: '7d' }, { force: true })} />
              : workEvents.length === 0 ? <p className="text-xs text-muted">No matching decisions were reported. This does not establish whether another factory is running.</p>
              : <ol className="space-y-3">
                {workEvents.map((event, index) => (
                  <li key={`${event.timestamp}-${index}`} className="break-words text-xs">
                    <div className="flex flex-wrap justify-between gap-2">
                      <span className="font-medium text-primary">{event.event_type.replaceAll('_', ' ')}</span>
                      <time dateTime={event.timestamp} className="text-muted">{formatLocalTime(event.timestamp) ?? event.timestamp}</time>
                    </div>
                    <p className="mt-1 whitespace-pre-wrap text-secondary">{event.details}</p>
                    {event.reason_code && <p className="mt-1 text-muted">Reason: {event.reason_code}</p>}
                    {(event.skipped ?? []).map((route, routeIndex) => (
                      <p key={routeIndex} className="mt-1 text-warning">{route.backend_instance ?? route.backend}{route.model ? ` / ${route.model}` : ''}: {route.reason}{route.unavailable_until ? ` · Retry after ${formatLocalTime(route.unavailable_until) ?? route.unavailable_until}` : ''}</p>
                    ))}
                  </li>
                ))}
              </ol>}
          </section>

          <section aria-labelledby="attempt-history-heading">
            <h3 id="attempt-history-heading" className="mb-3 text-sm font-semibold text-primary">Attempt history</h3>
            {timeline?.loading && !timeline.data ? (
              <LoadingState label="Loading attempt history…" />
            ) : timeline?.error ? (
              <ErrorState message={timeline.error} endpoint={`/api/work/${workId}`} onRetry={() => fetchWorkTimeline(workId, { profile, force: true })} />
            ) : entries.length === 0 ? (
              <div className="space-y-4">
                <WaypointHistory evidence={evidence} label={workId} />
                <EmptyState icon={Hammer} title="No ledger history for this work item yet" />
              </div>
            ) : (
              <div className="space-y-4">
                <WaypointHistory evidence={{ ...evidence, entries }} label={workId} />
                <AttemptTimeline entries={entries} />
              </div>
            )}
          </section>
        </div>
    </>
  );

  if (inline) {
    return (
      <>
      <section aria-labelledby="work-detail-title" className="flex min-w-0 flex-col">{body}</section>
      {reviewOpen && <CommitPrDialog profile={profile} sessionId={session?.id} nodeId={session?.nodeId} mergeRequest={mergeRequest} onClose={() => setReviewOpen(false)} onChanged={() => void refresh()} />}
      </>
    );
  }

  return (
    <>
    <dialog
      ref={dialog}
      aria-labelledby="work-detail-title"
      onCancel={onClose}
      onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}
      className="fixed inset-0 z-50 m-0 h-dvh max-h-none w-full max-w-none overflow-hidden border-0 bg-transparent p-0 backdrop:bg-scrim/35"
    >
      <aside
        className="ml-auto flex h-full w-full min-w-0 flex-col overflow-hidden border-l border-subtle bg-page shadow-2xl sm:max-w-2xl"
      >
        {body}
      </aside>
    </dialog>
    {reviewOpen && <CommitPrDialog profile={profile} sessionId={session?.id} nodeId={session?.nodeId} mergeRequest={mergeRequest} onClose={() => setReviewOpen(false)} onChanged={() => void refresh()} />}
    </>
  );
}
