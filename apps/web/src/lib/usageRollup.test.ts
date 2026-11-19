import type { RecordedUsageRow } from '@git-agent-harness/contracts';
import { observeUsage } from '@git-agent-harness/shared';
import { aggregateUsageRows } from './usageRollup.js';

const row: RecordedUsageRow = { source: 'dispatch', profile: 'project', project_provider: 'github', repo: 'owner/repo', work_id: '#42',
  backend: 'codex', backend_instance: 'work', account_label: null, auth_source_label: null, quota_pool: null, node_id: null, requested_model: 'alias', effective_model: 'alias', actual_model: 'actual', model_provider: 'provider',
  usage_classification: null, day: '2026-11-06', chat_turns: 0, dispatch_attempts: 1, legacy_dispatches: 0,
  measurements: observeUsage({ input_tokens: 10, output_tokens: 5, total_tokens: 100, cache_read_tokens: 80, cache_write_tokens: 0, reasoning_tokens: 3, actual_cost_usd: 0.25, estimated_cost_usd: 0 }) };

test('day grouping sums chat and factory readings once while retaining missing observations', () => {
  const chat = { ...row, source: 'chat' as const, chat_turns: 1, dispatch_attempts: 0, measurements: observeUsage({ total_tokens: 50, estimated_cost_usd: 0.1 }) };
  const [group] = aggregateUsageRows([row, chat], 'day');
  expect(group.chat_turns).toBe(1);
  expect(group.dispatch_attempts).toBe(1);
  expect(group.measurements.total_tokens.reported_total).toBe(150);
  expect(group.measurements.actual_cost_usd.reported_total).toBe(0.25);
  expect(group.measurements.actual_cost_usd.unmeasured_observations).toBe(1);
  expect(group.measurements.cache_read_tokens.reported_total).toBe(80);
  expect(group.measurements.cache_write_tokens.reported_total).toBe(0);
});

test('backend and model grouping preserve account/node and requested/actual identity', () => {
  const rows = [row, { ...row, backend_instance: 'personal' }, { ...row, node_id: 'worker' }];
  expect(aggregateUsageRows(rows, 'backend')).toHaveLength(3);
  expect(aggregateUsageRows([row, { ...row, auth_source_label: 'other-account' }], 'backend')).toHaveLength(2);
  expect(aggregateUsageRows([row, { ...row, quota_pool: 'other-pool' }], 'backend')).toHaveLength(2);
  expect(aggregateUsageRows([row, { ...row, actual_model: null }], 'model')).toHaveLength(2);
  expect(aggregateUsageRows([row, { ...row, requested_model: 'different' }], 'model')).toHaveLength(2);
});

test('project and work grouping preserve repository, forge and project identity', () => {
  const rows = [row, { ...row, profile: 'other' }, { ...row, project_provider: 'gitlab' }, { ...row, repo: 'owner/other' }];
  expect(aggregateUsageRows(rows, 'project')).toHaveLength(4);
  expect(aggregateUsageRows([...rows, { ...row, work_id: null }], 'work-item')).toHaveLength(5);
});
