import type { ControllerActivity } from '@git-agent-harness/contracts';

export type ControllerActivityDisplayStatus = ControllerActivity['status'] | 'deferred';

/** Historical dispatch errors include capacity refusals, which are not failed
 * agent runs. Preserve the recorded outcome, including any prior attempts. */
export function controllerActivityStatus(run: ControllerActivity): ControllerActivityDisplayStatus {
  if (run.status !== 'running' && /^[a-z_]+:\s*deferred_capacity:/i.test(run.outcome ?? '')) return 'deferred';
  return run.status;
}
