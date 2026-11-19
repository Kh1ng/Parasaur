import { expect, test } from '@playwright/experimental-ct-react';
import React from 'react';
import { readFileSync } from 'node:fs';
import type { RecordedUsageReport, RecordedUsageRow } from '@git-agent-harness/contracts';
import { addUsageMeasurements, emptyUsageMeasurements, observeUsage } from '@git-agent-harness/shared';
import { RecordedUsageCard } from '../../src/components/RecordedUsageCard.js';
import { WebSocketProvider } from '../../src/ws/WebSocketContext.js';

function fixture(): RecordedUsageReport {
  const dispatch: RecordedUsageRow = { source: 'dispatch', profile: 'fixture', project_provider: 'github', repo: 'team/project', work_id: '#42',
    backend: 'codex', backend_instance: 'work', account_label: null, auth_source_label: null, quota_pool: null, node_id: null, requested_model: 'route-model', effective_model: 'route-model', actual_model: 'reported-model',
    model_provider: 'provider', usage_classification: 'metered_api', day: '2026-11-06', chat_turns: 0, dispatch_attempts: 1, legacy_dispatches: 0,
    measurements: observeUsage({ input_tokens: 160, output_tokens: 40, total_tokens: 200, cache_read_tokens: 70, cache_write_tokens: 0, reasoning_tokens: 3, actual_cost_usd: 0.3, estimated_cost_usd: 0.2 }) };
  const chat: RecordedUsageRow = { ...dispatch, source: 'chat', project_provider: null, repo: null, node_id: 'worker', requested_model: null,
    actual_model: null, model_provider: null, usage_classification: null, chat_turns: 2, dispatch_attempts: 0,
    measurements: observeUsage({ input_tokens: 80, output_tokens: 20, total_tokens: 100, cache_read_tokens: 50, estimated_cost_usd: 0.1 }) };
  addUsageMeasurements(chat.measurements, observeUsage({}));
  const report: RecordedUsageReport = { schema_version: 1, scope: 'recorded-ledger-and-chat', profile: 'fixture',
    since: '2026-10-30T12:00:00Z', generated_at: '2026-11-06T12:00:00Z', sources: { chat: { status: 'available' }, dispatch: { status: 'available' } },
    rows: [chat, dispatch], totals: emptyUsageMeasurements() };
  for (const row of report.rows) addUsageMeasurements(report.totals, row.measurements);
  return report;
}

test.beforeEach(async ({ context }) => { await context.addInitScript({ content: 'window.__GAH_TEST_MODE__ = true' }); });

test('combined usage groups both sources, exports the same readings and keeps unknowns visible', async ({ mount, page }, testInfo) => {
  await page.route('**/api/usage/report?**', route => {
    const report = fixture();
    if (new URL(route.request().url()).searchParams.get('days') === '30') { report.rows = []; report.totals = emptyUsageMeasurements(); }
    return route.fulfill({ json: report });
  });
  const component = await mount(<WebSocketProvider><RecordedUsageCard profile="fixture" /></WebSocketProvider>);
  await expect(component.getByText('2 chat turns · 1 recorded dispatch attempt · 0 legacy summaries')).toBeVisible();
  await expect(component.getByText('Chat history available', { exact: true })).toBeVisible();
  await expect(component.getByText('Factory history available', { exact: true })).toBeVisible();
  const totals = component.locator('dl').first();
  await expect(totals.getByText('300', { exact: true })).toBeVisible();
  await expect(totals.getByText('1 missing', { exact: true })).toHaveCount(2);
  await expect(totals.getByText('2 missing', { exact: true })).toBeVisible();
  await expect(component.getByRole('table').getByRole('row')).toHaveCount(3);
  for (const grouping of ['model', 'project', 'work-item']) {
    await component.getByRole('combobox', { name: 'Group usage by' }).selectOption(grouping);
    await expect(component.getByRole('table').getByRole('row')).toHaveCount(3);
  }
  await component.getByRole('combobox', { name: 'Group usage by' }).selectOption('day');
  const row = component.getByRole('table').getByRole('row').nth(1);
  await expect(row.getByRole('cell').nth(1)).toHaveText('2');
  await expect(row.getByRole('cell').nth(2)).toHaveText('1');
  await expect(row.getByRole('cell').nth(4)).toHaveText('3001 missing');
  await row.getByText('Token categories', { exact: true }).click();
  await expect(row.getByText('cache read tokens', { exact: true }).locator('..')).toContainText('120');
  await expect(row.getByText('cache write tokens', { exact: true }).locator('..')).toContainText('0');
  await expect(component.getByRole('progressbar')).toHaveCount(0);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.screenshot({ path: testInfo.outputPath('combined-usage-desktop.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: testInfo.outputPath('combined-usage-narrow.png') });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  const downloading = page.waitForEvent('download');
  await component.getByRole('button', { name: 'Export JSON' }).click();
  const download = await downloading;
  expect(download.suggestedFilename()).toBe('parasaur-usage-7d.json');
  const exported = JSON.parse(readFileSync((await download.path())!, 'utf8')) as RecordedUsageReport;
  expect(exported.totals.total_tokens.reported_total).toBe(300);
  expect(exported.rows).toHaveLength(2);
  await component.getByRole('button', { name: '30d', exact: true }).click();
  await expect(component.getByText('No activity recorded', { exact: true })).toBeVisible();
});

test('a failed source preserves the other subtotal and never shows its missing counts as zero', async ({ mount, page }) => {
  let failingHttp = false;
  await page.route('**/api/usage/report?**', route => {
    if (failingHttp) return route.fulfill({ status: 503, json: { error: 'Report unavailable' } });
    const report = fixture();
    report.sources.dispatch = { status: 'unavailable', message: 'Factory history unavailable; retry after the node is restored.' };
    report.rows = report.rows.filter(row => row.source === 'chat');
    report.totals = report.rows[0].measurements;
    return route.fulfill({ json: report });
  });
  const component = await mount(<WebSocketProvider><RecordedUsageCard profile="fixture" /></WebSocketProvider>);
  await expect(component.getByText('Factory history unavailable; retry after the node is restored.')).toBeVisible();
  const row = component.getByRole('table').getByRole('row').nth(1);
  await expect(row.getByRole('cell').nth(2)).toHaveText('Unavailable');
  await expect(row.getByRole('cell').nth(3)).toHaveText('Unavailable');
  await expect(row.getByRole('cell').nth(4)).toHaveText('1001 missing');
  failingHttp = true;
  await component.getByRole('button', { name: 'Refresh recorded usage' }).click();
  await expect(component.getByRole('button', { name: 'Retry', exact: true })).toBeVisible();
  await expect(component.getByRole('table')).toHaveCount(0);
  failingHttp = false;
  await component.getByRole('button', { name: 'Retry', exact: true }).click();
  await expect(component.getByRole('table')).toBeVisible();
});
