import { dirname, join, resolve } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { readLoopExecutionState, runCheckNativePolicy } from './gahCli.js';
import type { NativeAuthorization, NativeDispatchBinding, NativeHostReservation } from './nativeDispatchBroker.js';

// A current-work CLI builds the same full evidence snapshot as the controller.
// One at a time bounds its transient memory across all profiles; do not cache
// old permission or retain a queue of snapshots while a node is disconnected.
let checking = false;

/** Concrete central policy check for both grant issue and consumption. The
 * broker separately fences the claim and reservation before/after this I/O.
 * No external-process fallback or permissive missing-CLI/loop behavior. */
export function nativeDispatchAuthority(root: string, centralUrl: string, config?: string) {
  const ownedRoot = resolve(root);
  return async (binding: NativeDispatchBinding, _reservation: NativeHostReservation, signal?: AbortSignal): Promise<NativeAuthorization> => {
    if (checking) return { allowed: false, reason: 'authority_busy' };
    checking = true;
    try {
      const run = resolve(ownedRoot, binding.run_id);
      if (dirname(run) !== ownedRoot || signal?.aborted) return { allowed: false, reason: 'authority_unavailable' };
      const before = await readLoopExecutionState(binding.profile, signal);
      if (before.state !== 'active') return { allowed: false, reason: before.state === 'paused' ? 'factory_paused' : 'authority_unavailable' };
      const current = await runCheckNativePolicy({ contextDirectory: join(run, 'context'), contextSha256: binding.context_sha256,
        nodeId: binding.node_id, centralUrl, config, signal, checkWork: true });
      if (!isDeepStrictEqual(current, binding)) return { allowed: false, reason: 'policy_changed' };
      const after = await readLoopExecutionState(binding.profile, signal);
      if (signal?.aborted) return { allowed: false, reason: 'authority_unavailable' };
      if (after.state !== 'active') return { allowed: false, reason: after.state === 'paused' ? 'factory_paused' : 'authority_unavailable' };
      if (!isDeepStrictEqual(before, after)) return { allowed: false, reason: 'controller_changed' };
      return { allowed: true };
    } catch { return { allowed: false, reason: 'authority_unavailable' }; }
    finally { checking = false; }
  };
}
