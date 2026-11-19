import { createHash } from 'node:crypto';
import { GAHError } from '@git-agent-harness/shared';
import type { SessionOptions } from './sessions/SessionManager.js';

/** Bind a replay to its original work, route and execution controls. A work
 * key alone deliberately omits controls and cannot establish request equality.
 * Operational receipt retention is not part of the requested execution. */
export function dispatchRequestFingerprint(options: SessionOptions & { nodeId?: string; coordinatorNodeId?: string }): string {
  const { gitlab } = options;
  const fields = [options.profile, options.providerKind, options.instanceId,
    options.repo, options.branch, options.target, options.mr, options.mode,
    options.backend, options.backendInstance, options.model, options.budget,
    options.reasoningEffort, options.nodeId, options.coordinatorNodeId,
    options.dryRun, options.retries, options.allowDraftFail, options.prod,
    options.allowUnknownRedBaseline, options.escalate, options.manualWorker,
    gitlab && [gitlab.project, gitlab.kind, gitlab.iid, gitlab.webUrl]];
  return createHash('sha256').update(JSON.stringify(fields)).digest('hex');
}

/** An unknown historical binding remains inspectable, but cannot authorize
 * another execution or be represented as the receipt for changed controls. */
export function assertDispatchRequestIdentity(previous: string | undefined, requested: string): void {
  if (!previous) throw new GAHError('The existing dispatch predates request binding. Inspect its receipt; execution controls cannot be verified.', 'DISPATCH_REQUEST_UNVERIFIED');
  if (previous !== requested) throw new GAHError('Dispatch receipt does not match the requested work, route or execution controls.', 'DISPATCH_REQUEST_CONFLICT');
}
