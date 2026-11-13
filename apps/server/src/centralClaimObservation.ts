import type { CentralClaimObservation, NodeObservationSnapshot, NodeObservationState } from '@git-agent-harness/contracts';

/** Project only bounded, recognized public facts from a worker's status payload. */
export function parseNodeCentralClaims(value: unknown, profile: string | null, source_state: NodeObservationState): NonNullable<NodeObservationSnapshot['central_claim_observations']> {
  if (!profile || !value || typeof value !== 'object') return [];
  const node = value as Record<string, unknown>;
  if (typeof node.central_url !== 'string' || node.central_url.length > 2048) return [];
  let central: URL;
  try { central = new URL(node.central_url); } catch { return []; }
  if (!['http:', 'https:'].includes(central.protocol) || central.username || central.password || central.pathname !== '/' || central.search || central.hash) return [];
  let observation: CentralClaimObservation | undefined;
  const raw = node.central_claim_observation;
  if (raw && typeof raw === 'object') {
    const candidate = raw as Record<string, unknown>;
    if (candidate.state === 'not_observed' || candidate.state === 'unavailable') observation = { state: candidate.state };
    if (candidate.state === 'observed'
      && typeof candidate.observed_at === 'string' && candidate.observed_at.length <= 64 && Number.isFinite(Date.parse(candidate.observed_at))
      && (candidate.work_id === null || (typeof candidate.work_id === 'string' && candidate.work_id.length <= 512))) {
      observation = {
        state: 'observed', observed_at: candidate.observed_at, work_id: candidate.work_id,
        outcome: candidate.outcome === 'acquired' || candidate.outcome === 'refused' ? candidate.outcome : 'unknown'
      };
    }
  }
  return [{ profile, central_url: central.origin, observation, source_state }];
}
