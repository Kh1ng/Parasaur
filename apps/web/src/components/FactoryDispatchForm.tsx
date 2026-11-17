import { useEffect, useId, useState } from 'react';
import { Rocket } from 'lucide-react';
import type { ClientMessage, ManagerBackendInfo, NodeObservationSnapshot, ProviderKind, Session } from '@git-agent-harness/contracts';
import { generateProviderInstanceId, generateRequestId } from '@git-agent-harness/shared';
import { useWebSocket } from '../ws/WebSocketContext.js';
import { ProviderPicker } from './ProviderPicker.js';

const MODES = ['fix', 'improve', 'review', 'pm', 'experiment'] as const;
const CONTROL = 'block mt-1 w-full min-h-[44px] bg-raised border border-subtle rounded-md px-3 py-2 text-sm text-primary disabled:opacity-50';
type StartMessage = Extract<ClientMessage, { type: 'session.start' }>;
type ExecutionNode = Pick<NodeObservationSnapshot, 'node_id' | 'display_name' | 'state' | 'profile' | 'profiles' | 'backend_instances' | 'dispatch_backend_instances'>;
type Submission = { message: StartMessage; state: 'pending' | 'unknown' | 'accepted' | 'error'; attempt: number; error?: string; session?: Session };

/** Account names belong to one node. Freeze that full request until the server
 * acknowledges it; an uncertain retry sends the same receipt ID and selection. */
export function FactoryDispatchForm({ profile, repo, backends, nodes, accountSelectionSupported }: {
  profile: string;
  repo: string | null;
  backends: ManagerBackendInfo[];
  nodes: ExecutionNode[];
  accountSelectionSupported: boolean;
}) {
  const { sendMessage, isConnected, messages, sessions } = useWebSocket();
  const [mode, setMode] = useState<(typeof MODES)[number]>('fix');
  const [backend, setBackend] = useState<ProviderKind>('auto');
  const [nodeId, setNodeId] = useState('');
  const [backendInstance, setBackendInstance] = useState('');
  const [model, setModel] = useState('');
  const [target, setTarget] = useState('');
  const [submission, setSubmission] = useState<Submission | null>(null);
  const modelListId = useId();
  const locked = submission?.state === 'pending' || submission?.state === 'unknown';
  const node = nodes.find(item => item.node_id === nodeId);
  const accounts = node?.backend_instances.filter(item => item.logical_backend === backend) ?? [];
  const account = accounts.find(item => item.backend_instance === backendInstance);
  const canSelectAccount = accountSelectionSupported && !!node && node.dispatch_backend_instances === true && backend !== 'auto';
  const ready = (item: typeof accounts[number]) => item.enabled && (item.executable_resolved ?? item.executable_configured) && item.auth_ready !== false;
  const nodeReady = (item: ExecutionNode) => item.state === 'healthy'
    && (!item.profile || item.profile === profile) && (!item.profiles.length || item.profiles.includes(profile));
  const models = [...new Set((account ? [account] : accounts).flatMap(item => item.supported_models))];
  const selectionReady = (!nodeId || !!node && nodeReady(node))
    && (!backendInstance || canSelectAccount && !!account && ready(account));

  useEffect(() => {
    if (!submission || submission.state === 'accepted' || submission.state === 'error') return;
    const requestId = submission.message.requestId;
    const response = [...messages].reverse().find(({ message }) =>
      (message.type === 'session.started' || message.type === 'error') && message.requestId === requestId)?.message;
    if (response?.type === 'error') {
      setSubmission(current => current && { ...current, state: 'error', error: response.error });
      return;
    }
    const session = response?.type === 'session.started' ? response.session : sessions.find(item => item.requestId === requestId);
    if (session) setSubmission(current => current && { ...current, state: 'accepted', session });
  }, [messages, sessions, submission]);

  useEffect(() => {
    if (submission?.state !== 'pending') return;
    const timer = window.setTimeout(() => setSubmission(current => current?.state === 'pending'
      ? { ...current, state: 'unknown' } : current), 15_000);
    return () => window.clearTimeout(timer);
  }, [submission?.state, submission?.attempt]);

  const dispatch = () => {
    if (!isConnected || !repo || submission?.state === 'pending') return;
    if (submission?.state === 'unknown') {
      setSubmission({ ...submission, state: 'pending', attempt: submission.attempt + 1 });
      sendMessage(submission.message);
      return;
    }
    if (!selectionReady || !backends.find(item => item.id === backend)?.implemented) return;
    const message: StartMessage = {
      type: 'session.start', requestId: generateRequestId(), profile,
      providerKind: backend, instanceId: generateProviderInstanceId(backend, 0), repo, mode, backend,
      target: target.trim() || undefined, nodeId: nodeId || undefined,
      backendInstance: backendInstance || undefined, model: model.trim() || undefined,
    };
    setSubmission({ message, state: 'pending', attempt: 1 });
    sendMessage(message);
  };
  const edit = () => setSubmission(null);

  return (
    <section className="card-padded" aria-label="Queue work">
      <h3 className="text-sm font-semibold text-primary mb-3 flex items-center gap-2"><Rocket size={15} aria-hidden="true" />Queue work</h3>
      <form onSubmit={event => { event.preventDefault(); dispatch(); }}>
        <fieldset disabled={locked} className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3 min-w-0">
          <label className="text-xs text-muted min-w-0">Mode
            <select aria-label="Mode" value={mode} onChange={event => { edit(); setMode(event.target.value as typeof mode); }} className={CONTROL}>
              {MODES.map(value => <option key={value}>{value}</option>)}
            </select>
          </label>
          <div className="text-xs text-muted min-w-0">
            <span className="mb-1 block">Backend</span>
            <ProviderPicker backends={backends} selectedBackendId={backend} models={[]} currentModelId={null}
              reasoningEfforts={[]} currentReasoningEffortId={null} modelsLoaded busy={locked}
              variant="backend" triggerAriaLabel="Factory backend"
              onSelect={selection => { edit(); setBackend(selection.backendId as ProviderKind); setBackendInstance(''); setModel(''); }} />
          </div>
          <label className="text-xs text-muted min-w-0">Target (issue number or ticket path)
            <input value={target} onChange={event => { edit(); setTarget(event.target.value); }} placeholder="e.g. 148" className={CONTROL} />
          </label>
          <label className="text-xs text-muted min-w-0">Execution node
            <select aria-label="Execution node" value={nodeId} onChange={event => { edit(); setNodeId(event.target.value); setBackendInstance(''); setModel(''); }} className={CONTROL}>
              <option value="">Automatic fleet routing</option>
              {nodeId && !node && <option value={nodeId} disabled>{nodeId} — unavailable</option>}
              {nodes.map(item => <option key={item.node_id} value={item.node_id} disabled={!nodeReady(item)}>{item.display_name} ({item.state})</option>)}
            </select>
          </label>
          <label className="text-xs text-muted min-w-0">Account
            <select aria-label="Account" value={backendInstance} disabled={!canSelectAccount} onChange={event => { edit(); setBackendInstance(event.target.value); setModel(''); }} className={CONTROL}>
              <option value="">Configured routing</option>
              {backendInstance && !account && <option value={backendInstance} disabled>{backendInstance} — unavailable</option>}
              {accounts.map(item => <option key={item.backend_instance} value={item.backend_instance} disabled={!ready(item)}>
                {item.account_label ?? item.backend_instance} ({item.backend_instance}){!ready(item) ? ' — unavailable' : ''}
              </option>)}
            </select>
          </label>
          <label className="text-xs text-muted min-w-0">Model (optional)
            <input value={model} disabled={backend === 'auto'} list={modelListId} placeholder="Configured model"
              onChange={event => { edit(); setModel(event.target.value); }} className={CONTROL} />
            <datalist id={modelListId}>{models.map(value => <option key={value} value={value} />)}</datalist>
          </label>
        </fieldset>
        <div className="mt-3 flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <p className="text-xs text-muted max-w-prose">
            {!accountSelectionSupported ? 'Update central to select named factory accounts.'
              : backend === 'auto' || !nodeId ? 'Choose a backend and execution node to select a named account.'
              : !canSelectAccount ? 'Update this worker to select a named account.'
              : backendInstance ? `Pinned to ${backendInstance} on ${node?.display_name}. An unavailable account stops dispatch.`
              : 'Configured routing chooses an account on this node.'}
          </p>
          <button type="submit" disabled={!isConnected || !repo || submission?.state === 'pending'
            || submission?.state !== 'unknown' && (!selectionReady || !backends.find(item => item.id === backend)?.implemented)}
            className="btn-primary min-h-[44px] shrink-0">
            {submission?.state === 'pending' ? 'Awaiting confirmation…' : submission?.state === 'unknown' ? 'Retry same request' : 'Dispatch'}
          </button>
        </div>
      </form>
      {!repo && <p className="text-xs text-critical mt-2">No repo known for this profile yet — check Settings.</p>}
      {!isConnected && <p className="text-xs text-warning mt-2">Disconnected. Reconnect before dispatching or retrying.</p>}
      {!selectionReady && <p className="text-xs text-critical mt-2">The selected node or account is unavailable. Refresh status or choose another selection.</p>}
      {submission?.state === 'unknown' && <p role="status" className="text-sm text-warning mt-3">Confirmation timed out. Work may have started. Check sessions below or retry this same request.</p>}
      {submission?.state === 'error' && <p role="alert" className="text-sm text-critical mt-3 break-words">Dispatch failed: {submission.error}</p>}
      {submission?.state === 'accepted' && <p role="status" className={`text-sm mt-3 break-words ${submission.session?.status === 'error' ? 'text-critical' : 'text-good'}`}>
        {submission.session?.status === 'error' ? `Dispatch failed: ${submission.session.error ?? 'Check session details.'}` : `Dispatch confirmed: ${submission.session?.id}. Follow its progress below.`}
      </p>}
    </section>
  );
}
