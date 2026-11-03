import type { MergeRequest } from '@git-agent-harness/contracts';

/** Explain controller handoffs that intentionally launch no reviewer or fixer.
 * Mirrors controller/decision.rs; a receipt prevents repeat calls, never grants authority. */
export function reviewWaitReason(mergeRequest?: MergeRequest | null): string | null {
  if (!mergeRequest || mergeRequest.merged) return null;
  if (mergeRequest.classification === 'READY_FOR_HUMAN' && mergeRequest.merge_review_block_reason) {
    return `Automatic merge is waiting: ${mergeRequest.merge_review_block_reason}`;
  }
  if (!mergeRequest.source_sha || mergeRequest.source_sha !== mergeRequest.last_reviewed_head) return null;
  const needsRepair = mergeRequest.classification === 'CI_FAILED' || mergeRequest.classification === 'NEEDS_FIX';
  const repairAuthorized = mergeRequest.review_generation != null
    && mergeRequest.review_generation_status == null
    && (mergeRequest.review_verdict === 'NEEDS_FIX' || mergeRequest.review_verdict === 'REJECT');
  if (mergeRequest.classification === 'NEEDS_REVIEW'
    || (needsRepair && (mergeRequest.review_gate_reason === 'ci_failed_waiting_for_push' || !repairAuthorized))) {
    return 'This commit has already been reviewed. Automatic review is paused until a new commit is pushed; inspect the recorded verdict and evidence.';
  }
  return null;
}

/** Shared labels for the PR/MR summary surfaces (review panel, work drawer). */
export function ciLabelFor(mergeRequest?: MergeRequest | null): string {
  return mergeRequest?.ci_pending ? 'Pending' : mergeRequest?.ci_passed ? 'Passed' : mergeRequest ? 'Not passing' : 'Unknown';
}

export function stateLabelFor(mergeRequest?: MergeRequest | null, providerOpen = false, existing = false): string {
  const state = mergeRequest?.state ?? (providerOpen || existing ? 'open' : 'Not published');
  return state.toLowerCase().replaceAll('_', ' ');
}
