import { randomUUID } from 'node:crypto';
import type { RegisteredNode, RunningWorker } from '@git-agent-harness/contracts';
import { canonicalWorkId } from './claimsService.js';
import { linkTo } from './workerLink.js';

type Target = { work_id: string; profile?: string; run_id?: string; node_id?: string };
type Receipt = { profile: string; work_id: string; run_id: string };

function identifier(value: unknown): value is string {
  return typeof value === 'string' && value.trim().length > 0 && value.length <= 1024
    && !/[\x00-\x1f\x7f]/.test(value);
}

export function cancellationTarget(value: unknown): Target | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return;
  const body = value as Record<string, unknown>;
  if (Object.keys(body).some(key => !['work_id', 'profile', 'run_id', 'node_id'].includes(key))) return;
  const { work_id, profile, run_id, node_id } = body;
  if (!identifier(work_id) || (profile != null && !identifier(profile)) ||
    (run_id != null && !identifier(run_id)) || (node_id != null && !identifier(node_id))) return;
  if (node_id && (!profile || !run_id)) return;
  return { work_id, profile: profile ?? undefined, run_id: run_id ?? undefined, node_id: node_id ?? undefined };
}

interface DispatchNodes {
  localId: string;
  central: boolean;
  list: (profile?: string) => Promise<RunningWorker[]>;
  registered: (id: string) => RegisteredNode | undefined;
  local: (work: string, profile?: string, run?: string) => Promise<Receipt>;
}

/** Pin a work-only request before relaying it. The executing node checks the live invocation. */
export async function cancelFactoryDispatch(target: Target, nodes: DispatchNodes): Promise<Receipt> {
  if (!target.node_id && nodes.central) {
    const workId = canonicalWorkId(target.work_id);
    const matches = (await nodes.list(target.profile)).filter(row =>
      row.state === 'running' && row.cancellable && row.work_id && canonicalWorkId(row.work_id) === workId
      && (!target.run_id || row.run_id === target.run_id)
      && (!target.profile || row.profile === target.profile));
    if (matches.length > 1) throw Error('Several dispatches match. Select the run to cancel.');
    const [row] = matches;
    if (!row?.work_id) throw Error('No matching dispatch is visible.');
    target = { work_id: row.work_id, node_id: row.node_id ?? nodes.localId,
      run_id: row.run_id, profile: row.profile ?? target.profile };
  }
  if (!target.node_id || target.node_id === nodes.localId) {
    return nodes.local(target.work_id, target.profile, target.run_id);
  }

  const node = nodes.central && nodes.registered(target.node_id);
  if (!node || !target.profile || !target.run_id || !node.profiles?.includes(target.profile)) {
    throw Error('The executing node is unavailable for this project.');
  }
  const response = await linkTo(node).fetch('/api/dispatch/cancel', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'Idempotency-Key': randomUUID() },
    signal: AbortSignal.timeout(15_000),
    body: JSON.stringify(target),
  });
  if (response.status !== 202) throw Error('The executing node did not accept cancellation (HTTP ' + response.status + ').');
  const receipt = await response.json() as Receipt;
  if (receipt?.run_id !== target.run_id || receipt.profile !== target.profile ||
    !identifier(receipt.work_id) || canonicalWorkId(receipt.work_id) !== canonicalWorkId(target.work_id)) {
    throw Error('The executing node returned a different dispatch.');
  }
  return receipt;
}