import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseDispatchExecutionOptions, requiresDispatchExecutionOptions } from './dispatchExecutionOptions.js';

test('execution controls preserve false and zero while rejecting ambiguous preview requests', () => {
  const keys = ['dryRun', 'allowDraftFail', 'prod', 'allowUnknownRedBaseline', 'escalate', 'manualWorker'];
  for (const key of keys) {
    for (const value of [true, false]) assert.deepEqual(parseDispatchExecutionOptions({ [key]: value }), { [key]: value });
    for (const value of ['true', 'false', 0, 1, null]) assert.equal(parseDispatchExecutionOptions({ [key]: value }), 'invalid');
  }
  for (const retries of [0, 2, 0xffff_ffff]) assert.deepEqual(parseDispatchExecutionOptions({ retries }), { retries });
  for (const retries of [-1, 0.5, NaN, Infinity, '2', null, 0x1_0000_0000]) assert.equal(parseDispatchExecutionOptions({ retries }), 'invalid');
  assert.deepEqual(parseDispatchExecutionOptions({ env: { GAH_BINARY: 'unsafe' }, configPath: '/untrusted' }), {});
  assert.equal(requiresDispatchExecutionOptions({}), false);
  assert.equal(requiresDispatchExecutionOptions({ dryRun: false, manualWorker: true }), false);
  assert.equal(requiresDispatchExecutionOptions({ retries: 0 }), true);
  for (const key of keys.filter(key => key !== 'manualWorker')) assert.equal(requiresDispatchExecutionOptions({ [key]: true }), true);
});
