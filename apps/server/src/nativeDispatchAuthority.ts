import { dirname, join, resolve } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { randomUUID } from 'node:crypto';
import { unlinkSync } from 'node:fs';
import type { NativeWorkerCatalog,NativeHeldWork } from '@git-agent-harness/contracts';
import { writeFileAtomic } from './atomicFile.js';
import { readLoopExecutionState, runCheckNativePolicy } from './gahCli.js';
import type { NativeAuthorization, NativeDispatchBinding, NativeHostReservation } from './nativeDispatchBroker.js';

// A current-work CLI builds the same full evidence snapshot as the controller.
// One at a time bounds its transient memory across all profiles; do not cache
// old permission or retain a queue of snapshots while a node is disconnected.
let checking = false;

/** Concrete central policy check for both grant issue and consumption. The
 * broker separately fences the claim and reservation before/after this I/O.
 * No external-process fallback or permissive missing-CLI/loop behavior. */
type Catalog=(binding:NativeDispatchBinding,signal?:AbortSignal)=>Promise<NativeWorkerCatalog>;
type Holds=(profile:string)=>NativeHeldWork[];
export function nativeDispatchAuthority(root: string, centralUrl: string, config?: string, catalog?:Catalog,holds?:Holds) {
  return authority(root, centralUrl, config, true,catalog,holds);
}

/** Running work rechecks policy/spend and exact live route approvals without
 * demanding that it still lead the current queue. Claims and sequence fences
 * remain broker-owned checks on both sides of this I/O. */
export function nativeContinuationAuthority(root: string, centralUrl: string, config?: string,catalog?:Catalog,holds?:Holds) {
  return authority(root, centralUrl, config, false,catalog,holds);
}

function authority(root: string, centralUrl: string, config: string | undefined, checkWork: boolean,catalog?:Catalog,holds?:Holds) {
  const ownedRoot = resolve(root);
  return async (binding: NativeDispatchBinding, _reservation: NativeHostReservation, signal?: AbortSignal): Promise<NativeAuthorization> => {
    if (checking) return { allowed: false, reason: 'authority_busy' };
    checking = true;
    let workerCatalogFile:string|undefined;
    let owned:NativeHeldWork[]|undefined;
    const currentHolds=()=>holds?.(binding.profile).sort((left,right)=>left.run_id.localeCompare(right.run_id));
    try {
      const run = resolve(ownedRoot, binding.run_id);
      if (dirname(run) !== ownedRoot || signal?.aborted) return { allowed: false, reason: 'authority_unavailable' };
      const before = await readLoopExecutionState(binding.profile, signal);
      if (before.state !== 'active') return { allowed: false, reason: before.state === 'paused' ? 'factory_paused' : 'authority_unavailable' };
      if(catalog) {
        const current=await catalog(binding,signal);
        owned=currentHolds();
        if(!owned) return {allowed:false,reason:'authority_unavailable'};
        workerCatalogFile=join(run,`catalog-${randomUUID()}.json`);
        writeFileAtomic(workerCatalogFile,JSON.stringify({catalog:current,held_work:owned}),0o600,process.platform!=='win32');
      }
      const current = await runCheckNativePolicy({ contextDirectory: join(run, 'context'), contextSha256: binding.context_sha256,
        nodeId: binding.node_id, centralUrl, config, signal, checkWork, checkRoute: !checkWork,workerCatalogFile });
      if (!isDeepStrictEqual(current, binding)) return { allowed: false, reason: 'policy_changed' };
      if(owned && !isDeepStrictEqual(owned,currentHolds())) return {allowed:false,reason:'claim_lost'};
      const after = await readLoopExecutionState(binding.profile, signal);
      if (signal?.aborted) return { allowed: false, reason: 'authority_unavailable' };
      if (after.state !== 'active') return { allowed: false, reason: after.state === 'paused' ? 'factory_paused' : 'authority_unavailable' };
      if (!isDeepStrictEqual(before, after)) return { allowed: false, reason: 'controller_changed' };
      return { allowed: true };
    } catch { return { allowed: false, reason: 'authority_unavailable' }; }
    finally { if(workerCatalogFile) {try {unlinkSync(workerCatalogFile);}catch{/* Keep private evidence when cleanup fails. */}} checking = false; }
  };
}
