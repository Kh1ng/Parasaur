import { expect, test } from '@playwright/test';
import { openUsage } from './helpers/navigation.js';
import type { RecordedUsageReport, RecordedUsageRow } from '@git-agent-harness/contracts';
import { addUsageMeasurements, emptyUsageMeasurements, observeUsage } from '@git-agent-harness/shared';

const chat: RecordedUsageRow = {
  source: 'chat', profile: 'fixture', project_provider: null, repo: null, work_id: null,
  backend: 'claude', backend_instance: 'claude-work', account_label: null, auth_source_label: null,
  quota_pool: null, node_id: 'chat-worker', requested_model: null, effective_model: 'claude-opus-4-1',
  actual_model: null, model_provider: null, usage_classification: null, day: '2026-09-23',
  chat_turns: 3, dispatch_attempts: 0, legacy_dispatches: 0,
  measurements: observeUsage({ input_tokens: 4_000_000, output_tokens: 2_160_000, total_tokens: 6_160_000 })
};
for (const reading of Object.values(chat.measurements)) {
  reading.measured_observations *= chat.chat_turns;
  reading.unmeasured_observations *= chat.chat_turns;
}
const unknown: RecordedUsageRow = { ...chat, backend: 'codex', backend_instance: 'codex-work',
  effective_model: 'gpt-5.3', chat_turns: 1, measurements: observeUsage({}) };
const totals = emptyUsageMeasurements();
for (const row of [chat, unknown]) addUsageMeasurements(totals, row.measurements);
const usageReport: RecordedUsageReport = {
  schema_version: 1, scope: 'recorded-ledger-and-chat', profile: 'fixture',
  since: '2026-09-16T00:00:00Z', generated_at: '2026-09-23T00:00:00Z',
  sources: { chat: { status: 'available' }, dispatch: { status: 'available' } },
  rows: [chat, unknown], totals
};

test('empty dispatch telemetry keeps manager-chat usage and unavailable-turn identity visible', async ({ page }) => {
  await page.route('**/api/usage/report**', route => route.fulfill({ json: usageReport }));
  await page.route('**/api/report?**', route => route.fulfill({ json: { comparisons: [] } }));
  await page.route('**/api/report/series?**', route => route.fulfill({ json: { series: [], bucket: 'daily' } }));

  await page.goto('/');
  await openUsage(page, 'Telemetry');

  const usage = page.getByRole('region', { name: 'Recorded usage', exact: true });
  await expect(usage).toContainText('4 chat turns · 0 recorded dispatch attempts');
  await usage.getByRole('combobox', { name: 'Group usage by' }).selectOption('model');
  const measured = usage.getByRole('row').filter({ hasText: 'claude-opus-4-1' });
  await expect(measured.getByRole('cell').nth(1)).toHaveText('3');
  await expect(measured.getByRole('cell').nth(4)).toHaveText('6.2M');
  const unmeasured = usage.getByRole('row').filter({ hasText: 'gpt-5.3' });
  await expect(unmeasured).toContainText('codex-work');
  await expect(unmeasured).toContainText('chat-worker');
  await expect(unmeasured.getByRole('cell').nth(1)).toHaveText('1');
  await expect(unmeasured.getByRole('cell').nth(4)).toHaveText('Not reported1 missing');

  await expect(page.getByText('No dispatch-ledger trend', { exact: true })).toBeVisible();
  await expect(page.getByText('No dispatch-ledger data', { exact: true })).toBeVisible();

  await page.reload();
  await openUsage(page, 'Telemetry');
  await usage.getByRole('combobox', { name: 'Group usage by' }).selectOption('model');
  await expect(measured.getByRole('cell').nth(4)).toHaveText('6.2M');

  await page.setViewportSize({ width: 390, height: 844 });
  await expect(unmeasured).toContainText('codex-work');
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth)).toBe(true);
});
