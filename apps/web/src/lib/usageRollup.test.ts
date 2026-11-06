import type { UsageRollupRow } from '@git-agent-harness/contracts';
import { aggregateUsageRows } from './usageRollup.js';

const row: UsageRollupRow = { backend: 'codex', backend_instance: 'work', model: 'test', day: '2026-11-06', turns: 2,
  input_tokens: 10, output_tokens: 5, total_tokens: 100, cache_read_tokens: 80, cache_write_tokens: 0,
  reasoning_tokens: 3, estimated_cost_usd: 0 };

test('account and model rollups preserve independent categories and observed zero', () => {
  const result = aggregateUsageRows([row, { ...row, day: '2026-11-05' }, { ...row, backend_instance: 'personal', total_tokens: 7 }]);
  expect(result).toHaveLength(2);
  expect(result[0][1]).toMatchObject({ turns: 4, totalTokens: 200, cacheReadTokens: 160, cacheWriteTokens: 0, reasoningTokens: 6, estimatedCostUsd: 0 });
  expect(result[0][1].models.get('test')?.totalTokens).toBe(200);
  expect(result[1][1].totalTokens).toBe(7);
});

test('a missing day or legacy category invalidates only its corresponding total', () => {
  const result = aggregateUsageRows([row, { ...row, total_tokens: null, cache_read_tokens: undefined, estimated_cost_usd: null }])[0][1];
  expect(result.totalTokens).toBeNull();
  expect(result.cacheReadTokens).toBeNull();
  expect(result.inputTokens).toBe(20);
  expect(result.outputTokens).toBe(10);
  expect(result.estimatedCostUsd).toBeNull();
  expect(result.models.get('test')?.totalTokens).toBeNull();
});

test('display labels cannot merge different backend and account identities', () => {
  expect(aggregateUsageRows([row, { ...row, backend: 'codex · work', backend_instance: null }])).toHaveLength(2);
  expect(aggregateUsageRows([row, { ...row, model: null }])[0][1].models.size).toBe(2);
});
