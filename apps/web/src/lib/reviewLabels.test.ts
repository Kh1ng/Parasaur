import type { MergeRequest } from '@git-agent-harness/contracts';
import { ciLabelFor, stateLabelFor, reviewWaitReason } from './reviewLabels.js';

const mr: MergeRequest = {
  branch: 'fix', id: '1', url: null, state: 'REVIEW_REQUIRED', draft: false,
  merge_status: null, merged: false, ci_passed: true, ci_pending: false,
  review_contract_version: 1, classification: 'ready', recommended_action: 'NONE',
};

test('a completed review explains why the unchanged commit will not be reviewed again', () => {
  const reviewed = { ...mr, classification: 'NEEDS_REVIEW', source_sha: 'head-one', last_reviewed_head: 'head-one' };
  expect(reviewWaitReason(reviewed)).toContain('paused until a new commit is pushed');
  expect(reviewWaitReason({ ...reviewed, source_sha: 'head-two' })).toBeNull();
  expect(reviewWaitReason({ ...reviewed, last_reviewed_head: null })).toBeNull();
  expect(reviewWaitReason({ ...reviewed, merged: true })).toBeNull();
});

test('a current repair contract permits fixing; a repeat-review receipt alone does not', () => {
  const failed = { ...mr, classification: 'CI_FAILED', source_sha: 'head-one', last_reviewed_head: 'head-one' };
  expect(reviewWaitReason(failed)).not.toBeNull();
  const repair = { ...failed, review_verdict: 'NEEDS_FIX', review_generation: 'current', review_generation_status: null };
  expect(reviewWaitReason(repair)).toBeNull();
  expect(reviewWaitReason({ ...repair, review_generation_status: 'source_changed' })).not.toBeNull();
  expect(reviewWaitReason({ ...repair, review_gate_reason: 'ci_failed_waiting_for_push' })).not.toBeNull();
});

test('a current-head veto explains merge deferral without a durable hold', () => {
  expect(reviewWaitReason({ ...mr, classification: 'READY_FOR_HUMAN', merge_review_block_reason: 'changes_requested' }))
    .toBe('Automatic merge is waiting: changes_requested');
});
test('labels CI with pending taking precedence over passed', () => {
  expect(ciLabelFor(mr)).toBe('Passed');
  expect(ciLabelFor({ ...mr, ci_pending: true })).toBe('Pending');
  expect(ciLabelFor({ ...mr, ci_passed: false })).toBe('Not passing');
});
test('missing CI evidence stays unknown', () => {
  expect(ciLabelFor()).toBe('Unknown');
  expect(ciLabelFor(null)).toBe('Unknown');
});
test('formats provider state and falls back to publication evidence', () => {
  expect(stateLabelFor(mr)).toBe('review required');
  expect(stateLabelFor({ ...mr, state: null })).toBe('not published');
  expect(stateLabelFor(null, true)).toBe('open');
  expect(stateLabelFor(undefined, false, true)).toBe('open');
  expect(stateLabelFor()).toBe('not published');
});
