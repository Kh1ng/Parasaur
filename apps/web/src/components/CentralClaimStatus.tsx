import { useId } from 'react';
import type { CentralClaimObservation, NodeObservationState } from '@git-agent-harness/contracts';
import { formatLocalTime } from '../lib/format.js';

/** Admission history is deliberately separate from controller gates and current lease ownership. */
export function CentralClaimStatus({ observation, nodeName, centralUrl, sourceState }: { observation?: CentralClaimObservation; nodeName?: string; centralUrl?: string; sourceState?: NodeObservationState }) {
  const titleId = useId();
  const observed = observation?.state === 'observed' ? observation : null;
  const refused = observed?.outcome === 'refused';
  const acquired = observed?.outcome === 'acquired';
  const title = refused ? 'Latest central claim refused' : acquired ? 'Latest central claim acquired' : 'Central claim evidence unavailable';
  const message = refused
    ? 'That dispatch did not start. Check central connectivity, node access, or a competing claim before retrying.'
    : acquired
      ? 'The central granted this attempt. This does not establish a current lease or current service health.'
      : observation?.state === 'not_observed'
        ? 'No claim attempt has been recorded for this project and central. Dispatch still requires a central lease.'
        : observation?.state === 'unavailable'
          ? 'The local claim history could not be read. Check the node’s event log before drawing conclusions.'
          : 'This node has not reported a recognized claim result. Update or inspect the node for admission evidence.';
  return <section aria-labelledby={titleId} className="card-padded min-w-0 space-y-2 [overflow-wrap:anywhere]">
    <h3 id={titleId} className={`text-sm font-semibold ${refused ? 'text-warning' : 'text-primary'}`}>{nodeName ? `${nodeName}: ${title}` : title}</h3>
    {centralUrl && <p className="break-all text-xs text-muted">Central: {centralUrl}</p>}
    {sourceState && sourceState !== 'healthy' && <p className="text-xs text-warning">Node observation: {sourceState}. Showing the last reported claim evidence.</p>}
    <p className="max-w-prose text-sm text-secondary">{message}</p>
    {observed && <p className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted">
      {observed.work_id && <span className="font-mono">{observed.work_id}</span>}
      <span>Observed <time dateTime={observed.observed_at} title={observed.observed_at}>{formatLocalTime(observed.observed_at) ?? 'at an unknown time'}</time></span>
    </p>}
  </section>;
}
