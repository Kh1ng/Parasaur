import type { LoopDecision, NodeFactoryBlocker, NodeFactoryRead, NodeFactorySnapshot, RegisteredNode } from '@git-agent-harness/contracts';

export class NodeFactoryError extends Error {
  constructor(public readonly status: number, message: string) { super(message); }
}
const record = (value: unknown): Record<string, unknown> => value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
const text = (value: unknown, limit = 1024) => typeof value === 'string' ? value.slice(0, limit) : null;
const timestamp = (value: unknown) => typeof value === 'string' && value.length <= 64 && Number.isFinite(Date.parse(value));
function blocker(value: unknown): NodeFactoryBlocker {
  const item = record(value), plan = record(item.remediation_plan);
  return { work_id: text(plan.work_id ?? item.source_reference, 512), reason_code: text(item.reason_code, 128), message: text(item.message),
    next_steps: Array.isArray(plan.safe_actions) ? plan.safe_actions.slice(0, 3).map(action => text(record(action).summary)).filter((step): step is string => step !== null) : [] };
}

/** Read only fixed endpoints on the registered origin, with one network/body
 * deadline and bounded projected output. Never relay remote mutation commands. */
export async function readNodeFactory(node: RegisteredNode, profile: string, headers: Record<string, string>): Promise<NodeFactorySnapshot> {
  const origin = new URL(node.advertised_url);
  const loopback = ['localhost', '127.0.0.1', '[::1]'].includes(origin.hostname) || /^127\./.test(origin.hostname);
  if (!['http:', 'https:'].includes(origin.protocol) || origin.username || origin.password
    || origin.protocol === 'http:' && !loopback && node.transport_mode !== 'trusted_lan') throw new NodeFactoryError(400, 'Worker transport is not permitted.');
  const signal = AbortSignal.timeout(5000);
  async function read<T>(path: string, project: (value: unknown) => T): Promise<NodeFactoryRead<T>> {
    try {
      const url = new URL(path, origin); url.searchParams.set('profile', profile);
      const response = await fetch(url, { headers, signal, redirect: 'error' });
      if (!response.ok) {
        await response.body?.cancel();
        return { state: 'unavailable', reason: [401, 403].includes(response.status) ? 'authorization' : response.status === 404 ? 'unsupported' : 'transport' };
      }
      const reader = response.body?.getReader();
      if (!reader) return { state: 'unavailable', reason: 'invalid_response' };
      const chunks: Uint8Array[] = []; let bytes = 0;
      try {
        while (true) {
          const { done, value } = await reader.read(); if (done) break;
          bytes += value.byteLength;
          if (bytes > 2 * 1024 * 1024) { await reader.cancel(); return { state: 'unavailable', reason: 'invalid_response' }; }
          chunks.push(value);
        }
      } finally { reader.releaseLock(); }
      try { return { state: 'available', value: project(JSON.parse(Buffer.concat(chunks).toString('utf8'))) }; }
      catch { return { state: 'unavailable', reason: 'invalid_response' }; }
    } catch { return { state: 'unavailable', reason: signal.aborted ? 'timeout' : 'transport' }; }
  }
  const [status, loop, decision] = await Promise.all([
    read('/api/status', value => {
      const v = record(value);
      if (record(v.profile).profile !== profile || !timestamp(v.generated_at) || !Array.isArray(v.blockers) || !Array.isArray(v.blocked_work_items)) throw new Error('Invalid status');
      return { generated_at: v.generated_at as string, profile_blockers: v.blockers.slice(0, 50).map(blocker), profile_blocker_count: v.blockers.length,
        work_blockers: v.blocked_work_items.slice(0, 50).map(blocker), work_blocker_count: v.blocked_work_items.length };
    }),
    read('/api/loop/status', value => {
      const v = record(value); if (typeof v.running !== 'boolean') throw new Error('Invalid loop');
      return { running: v.running };
    }),
    read<LoopDecision | null>('/api/loop/last-decision', value => {
      if (value === null) return null;
      const v = record(value);
      if (!timestamp(v.timestamp) || typeof v.kind !== 'string' || typeof v.reason !== 'string') throw new Error('Invalid decision');
      return { timestamp: v.timestamp as string, kind: text(v.kind, 128)!, reason: text(v.reason)!, work_id: text(v.work_id, 512), reason_code: text(v.reason_code, 128) };
    }),
  ]);
  return { node_id: node.node_id, profile, checked_at: new Date().toISOString(), status, loop, decision };
}
