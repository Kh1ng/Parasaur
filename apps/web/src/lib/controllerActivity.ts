import type { ControllerActivity } from '@git-agent-harness/contracts';

export type ControllerActivityDisplayStatus = ControllerActivity['status'] | 'deferred';

/** Historical dispatch errors include capacity refusals, which are not failed
 * agent runs. Preserve the recorded outcome, including any prior attempts. */
export function controllerActivityStatus(run: ControllerActivity): ControllerActivityDisplayStatus {
  if (run.status !== 'running' && /^[a-z_]+:\s*deferred_capacity:/i.test(run.outcome ?? '')) return 'deferred';
  return run.status;
}

/** A terminal refusal explains the last attempt, not current node eligibility.
 * Keep prior-attempt evidence; the route-state fingerprint is diagnostic only. */
export function controllerCapacityDeferral(run: ControllerActivity | undefined): { summary: string; detail: string; nextStep: string } | null {
  if (!run || controllerActivityStatus(run) !== 'deferred') return null;
  const detail = run.outcome!.replace(/^[a-z_]+:\s*deferred_capacity:\s*/i, '').replace(/\s+route_state=\S+\s*$/, '');
  return {
    summary: detail.includes('node admission deferred') ? 'Last attempt waited for node capacity' : 'Last attempt waited for provider capacity',
    detail,
    nextStep: detail.includes('node admission deferred')
      ? 'Check the execution node’s resources and worker reservations. Capacity is checked again before launch.'
      : 'Inspect the skipped routes and their account sign-in, quota and concurrency limits.'
  };
}
