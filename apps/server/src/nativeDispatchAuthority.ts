import { dirname, join, resolve } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { randomUUID } from 'node:crypto';
import { unlinkSync } from 'node:fs';
import type { NativeWorkerCatalog,NativeHeldWork } from '@git-agent-harness/contracts';
import { writeScratchFile } from './atomicFile.js';
import { readLoopExecutionState, runCheckNativePolicy } from './gahCli.js';
import type { NativeAuthorization, NativeDispatchBinding, NativeHostReservation } from './nativeDispatchBroker.js';

// A current-work CLI builds the same full evidence snapshot as the controller.
// One at a time bounds its transient memory across all profiles; do not cache
// old permission or retain a queue of snapshots while a node is disconnected.
// The route check builds no snapshot, so it neither takes nor waits for the
// slot: a worker consuming its start must not be refused because another
// run's selection is still being checked.
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
    if (checkWork) {
      if (checking) return { allowed: false, reason: 'authority_busy' };
      checking = true;
    }
    let workerCatalogFile:string|undefined;
    let owned:NativeHeldWork[]|undefined;
    let stage = 'read controller state';
    const currentHolds=()=>holds?.(binding.profile).sort((left,right)=>left.run_id.localeCompare(right.run_id));
    try {
      const run = resolve(ownedRoot, binding.run_id);
      if (dirname(run) !== ownedRoot || signal?.aborted) return { allowed: false, reason: 'authority_unavailable' };
      const before = await readLoopExecutionState(binding.profile, signal);
      if (before.state !== 'active') return { allowed: false, reason: before.state === 'paused' ? 'factory_paused' : 'authority_unavailable' };
      if(catalog) {
        stage = 'read worker catalog and held work';
        const current=await catalog(binding,signal);
        owned=currentHolds();
        if(!owned) return {allowed:false,reason:'authority_unavailable'};
        // Read once by the check below, then deleted. Making it durable cost
        // two disk flushes on the server's only thread for every start and
        // every operation a worker asked permission for.
        workerCatalogFile=join(run,`catalog-${randomUUID()}.json`);
        writeScratchFile(workerCatalogFile,JSON.stringify({catalog:current,held_work:owned}));
      }
      stage = checkWork ? 'check selected work and policy' : 'check running route and policy';
      const current = await runCheckNativePolicy({ contextDirectory: join(run, 'context'), contextSha256: binding.context_sha256,
        nodeId: binding.node_id, centralUrl, config, signal, checkWork, checkRoute: !checkWork,workerCatalogFile });
      if (!isDeepStrictEqual(current, binding)) return { allowed: false, reason: 'policy_changed' };
      if(owned && !isDeepStrictEqual(owned,currentHolds())) return {allowed:false,reason:'claim_lost'};
      stage = 'recheck controller state';
      const after = await readLoopExecutionState(binding.profile, signal);
      if (signal?.aborted) return { allowed: false, reason: 'authority_unavailable' };
      if (after.state !== 'active') return { allowed: false, reason: after.state === 'paused' ? 'factory_paused' : 'authority_unavailable' };
      if (!isDeepStrictEqual(before, after)) return { allowed: false, reason: 'controller_changed' };
      return { allowed: true };
    } catch {
      // CLI errors can contain profile settings. Log only the known operation.
      console.warn(`Native authority unavailable for run ${binding.run_id} during ${stage}${signal?.aborted ? ' (deadline exceeded)' : ''}`);
      return { allowed: false, reason: 'authority_unavailable' };
    }
    finally { if(workerCatalogFile) {try {unlinkSync(workerCatalogFile);}catch{/* Keep private evidence when cleanup fails. */}} if (checkWork) checking = false; }
  };
}
