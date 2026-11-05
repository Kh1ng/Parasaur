import { useEffect, useRef, useState } from 'react';
import type { NodeFactoryBlocker, NodeFactoryRead, NodeFactorySnapshot } from '@git-agent-harness/contracts';
import { gahApi } from '../api/client.js';

const unavailable: Record<Extract<NodeFactoryRead<unknown>, { state: 'unavailable' }>['reason'], string> = {
  authorization: 'Worker access was denied. Check its registered credential on central.',
  unsupported: 'This worker does not provide this observation. Check its installed version.',
  timeout: 'The worker did not finish responding within five seconds.',
  transport: 'Cannot read this observation. Check the worker connection.',
  invalid_response: 'The worker returned an invalid, oversized or mismatched observation.',
};
function GateList({ label, items, count }: { label: string; items: NodeFactoryBlocker[]; count: number }) {
  return <div className="space-y-2">
    <h5 className="text-sm font-medium text-primary">{label} ({count})</h5>
    {count === 0 ? <p className="text-sm text-secondary">None reported at this check.</p> : <ul className="space-y-3">{items.map((item, index) => <li key={index} className="break-words text-sm space-y-1">
      <p className="font-medium text-primary">{item.work_id ?? 'Profile or unidentified work'}{item.reason_code ? ` · ${item.reason_code.replaceAll('_', ' ')}` : ''}</p>
      <p className="text-secondary">{item.message ?? 'The worker did not provide a readable explanation.'}</p>
      {item.next_steps.length > 0 && <ul className="list-disc pl-5 text-secondary">{item.next_steps.map((step, i) => <li key={i}>{step}</li>)}</ul>}
    </li>)}</ul>}
    {count > items.length && <p className="text-sm text-warning">Showing {items.length} of {count} reported gates.</p>}
  </div>;
}

/** Explicit read-only worker check; actions here must never target the central ledger. */
export function NodeFactoryDetails({ nodeId, profiles }: { nodeId: string; profiles: string[] }) {
  const [profile, setProfile] = useState(profiles[0] ?? '');
  const [result, setResult] = useState<NodeFactorySnapshot | null>(null);
  const [busy, setBusy] = useState(false), [error, setError] = useState('');
  const request = useRef(0);
  useEffect(() => () => { ++request.current; }, []);
  const check = async () => {
    const sequence = ++request.current;
    setBusy(true); setError('');
    try {
      const next = await gahApi.getNodeFactory(nodeId, profile);
      if (sequence !== request.current) return;
      if (next.node_id !== nodeId || next.profile !== profile) throw new Error('Worker or profile did not match this request.');
      setResult(next);
    } catch (failure) {
      if (sequence === request.current) setError(failure instanceof Error ? failure.message : 'Cannot read worker factory state.');
    } finally { if (sequence === request.current) setBusy(false); }
  };
  return <section aria-label="Worker factory state" className="space-y-3 border-y border-subtle py-4">
    <h4 className="font-medium text-primary">Worker factory state</h4>
    <p className="text-sm text-secondary">Read this worker’s controller and reported gates. These observations do not confirm that an agent is executing.</p>
    <div className="flex flex-wrap items-end gap-3">
      <label className="text-sm text-secondary">Worker profile<select className="input mt-1 block" value={profile} onChange={event => {
        ++request.current; setProfile(event.target.value); setResult(null); setError(''); setBusy(false);
      }}>{profiles.map(name => <option key={name} value={name}>{name}</option>)}</select></label>
      <button type="button" className="btn-secondary min-h-11" disabled={busy || !profile} onClick={() => void check()}>{busy ? 'Reading factory state…' : 'Read factory state'}</button>
    </div>
    {!profile && <p className="text-sm text-warning">Register a profile on this worker before reading its factory state.</p>}
    {error && <p role="alert" className="text-sm text-critical">{error}{result && ' The previous check is retained below; current state is unconfirmed.'}</p>}
    {result && <div className="space-y-4">
      <p className="text-sm text-muted">{busy || error ? 'Last recorded check' : 'Checked'}: {result.checked_at} · {result.profile} · {result.node_id}</p>
      <div className="space-y-1"><h5 className="text-sm font-medium text-primary">Controller at this check</h5>
        <p className="text-sm text-secondary">{result.loop.state === 'available' ? result.loop.value.running ? 'Controller reported running.' : 'Controller reported stopped.' : unavailable[result.loop.reason]}</p>
      </div>
      <div className="space-y-1"><h5 className="text-sm font-medium text-primary">Last decision in the worker’s 24-hour event window</h5>
        {result.decision.state === 'unavailable' ? <p className="text-sm text-warning">{unavailable[result.decision.reason]}</p>
          : result.decision.value ? <><p className="break-words text-sm text-primary">{result.decision.value.work_id} · {result.decision.value.kind.replaceAll('_', ' ')} · {result.decision.value.timestamp}</p><p className="break-words text-sm text-secondary">{result.decision.value.reason}</p></>
            : <p className="text-sm text-secondary">No decision recorded in this window. This does not prove the factory is idle.</p>}
      </div>
      {result.status.state === 'unavailable' ? <p className="text-sm text-warning">Factory gates: {unavailable[result.status.reason]}</p> : <>
        <p className="text-sm text-muted">Gate snapshot generated: {result.status.value.generated_at}</p>
        <GateList label="Profile gates" items={result.status.value.profile_blockers} count={result.status.value.profile_blocker_count} />
        <GateList label="Work awaiting action" items={result.status.value.work_blockers} count={result.status.value.work_blocker_count} />
      </>}
    </div>}
  </section>;
}
