import { useEffect, useState } from 'react';
import type { RunningWorker } from '@git-agent-harness/contracts';
import { gahApi } from '../api/client.js';
import { StatusBadge } from './ui/StatusBadge.js';
import { formatDuration } from './LiveAgentsCard.js';

function CancelDispatch({worker}: {worker: RunningWorker}) {
  const [state,setState] = useState<'ready' | 'sending' | 'requested'>('ready');
  const [error,setError] = useState('');
  const { work_id, profile } = worker;
  if (!work_id || !profile || !worker.cancellable || worker.state !== 'running') return null;
  const cancel = async () => {
    setState('sending'); setError('');
    try {
      await gahApi.cancelDispatch({work_id, profile, run_id:worker.run_id,
        ...(worker.node_id ? {node_id:worker.node_id} : {})});
      setState('requested');
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Cancellation could not be confirmed.');
      setState('ready');
    }
  };
  return <>
    <button type="button" className="text-accent" onClick={cancel} disabled={state !== 'ready'}>
      {state === 'requested' ? 'Cancellation requested' : state === 'sending' ? 'Cancelling…' : 'Cancel dispatch'}
    </button>
    {error && <span role="alert" className="text-xs text-critical">{error}</span>}
  </>;
}

export function RunningWorkersRoster({ workers, onOpenWork, onWatch }: {
  workers: RunningWorker[];
  onOpenWork?: (workId: string) => void;
  onWatch?: (worker: RunningWorker) => void;
}) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => { const timer = window.setInterval(() => setNow(Date.now()), 1000); return () => window.clearInterval(timer); }, []);
  return <section id="running-workers" aria-label="Running workers" className="min-w-0 space-y-3">
    <h3 className="text-sm font-semibold text-primary">Running workers ({workers.length})</h3>
    {workers.length === 0 ? <p className="text-sm text-muted">No running workers reported.</p> :
      <ul className="card divide-y divide-subtle" aria-label="Worker roster">{workers.map(worker => <li data-testid="worker-row" key={`${worker.node_id}:${worker.run_id}:${worker.attempt}`} className="min-w-0 space-y-2 p-3 text-sm break-words [overflow-wrap:anywhere]">
        <div className="flex flex-wrap items-center gap-2">
          <StatusBadge tone={worker.state === 'stale' ? 'warning' : 'good'} label={worker.state} />
          {onOpenWork && worker.work_id ? <button className="text-accent" onClick={() => onOpenWork(worker.work_id!)}>{worker.work_id}</button> : <span>{worker.work_id ?? 'Unknown work item'}</span>}
          <span>{worker.mode} · attempt {worker.attempt}</span>
          <CancelDispatch worker={worker} />
          {onWatch && <button className="text-accent" aria-label={`Watch ${worker.work_id ?? worker.run_id} live`} onClick={() => onWatch(worker)}>Watch output</button>}
        </div>
        <p>Runner: {worker.runner} · Backend: {worker.backend} · Model: {worker.model ?? 'Unknown'}</p>
        <p className="text-xs text-muted">Node: {worker.node_id ? <a className="text-accent" href={`?page=nodes${worker.profile ? `&profile=${encodeURIComponent(worker.profile)}` : ''}`}>{worker.node_id}</a> : 'Unknown'} · Instance: {worker.backend_instance} · Branch: {worker.branch ?? 'Unknown'} · Session: {worker.run_id}</p>
        <p className="text-xs text-muted">Started: {worker.started_at} · Elapsed: {formatDuration(Math.max(0, now - Date.parse(worker.started_at)))} · Last activity: {worker.last_activity_at}</p>
      </li>)}</ul>}
  </section>;
}
