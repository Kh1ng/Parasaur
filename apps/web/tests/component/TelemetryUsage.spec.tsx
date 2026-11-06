import { expect, test } from '@playwright/experimental-ct-react';
import React from 'react';
import type { UsageRollupRow, UsageRollupSummary } from '@git-agent-harness/contracts';
import { ChatUsageRollupCard } from '../../src/pages/TelemetryPage.js';
import { WebSocketProvider } from '../../src/ws/WebSocketContext.js';

test('usage shows independent categories, unknown totals and observed zero without invented cache', async ({ mount, page, context }, testInfo) => {
  await context.addInitScript({ content: 'window.__GAH_TEST_MODE__ = true' });
  const row: UsageRollupRow = { backend: 'codex', backend_instance: 'work', model: 'test', day: '2026-11-06', turns: 1,
    input_tokens: 10, output_tokens: 5, total_tokens: 100, cache_read_tokens: 80, cache_write_tokens: 0,
    reasoning_tokens: 3, estimated_cost_usd: 0 };
  await page.route('**/api/usage/rollup?**', route => {
    const rows = new URL(route.request().url()).searchParams.get('days') === '30'
      ? [row] : [row, { ...row, day: '2026-11-05', total_tokens: null, estimated_cost_usd: null }];
    const body: UsageRollupSummary = { profile: 'fixture', since: 0, generated_at: 1, rows, unattributed_turns: 0, usage_unavailable: [], tickets: [] };
    return route.fulfill({ json: body });
  });
  const component = await mount(<WebSocketProvider><ChatUsageRollupCard profile="fixture" /></WebSocketProvider>);
  const account = component.getByRole('row').filter({ has: page.getByText('codex · work', { exact: true }) });
  await expect(account).toBeVisible();
  const cells = account.getByRole('cell');
  await expect(cells.nth(2)).toHaveText('20');
  await expect(cells.nth(3)).toHaveText('10');
  await expect(cells.nth(4)).toHaveText('160');
  await expect(cells.nth(5)).toHaveText('0');
  await expect(cells.nth(6)).toHaveText('6');
  await expect(cells.nth(7)).toHaveText('Not reported');
  await expect(cells.nth(8)).toHaveText('Not reported');
  await expect(component.getByRole('progressbar')).toHaveCount(0);
  await expect(component.getByText('Cache / other', { exact: true })).toHaveCount(0);
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.screenshot({ path: testInfo.outputPath('usage-desktop.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: testInfo.outputPath('usage-narrow.png') });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await component.getByRole('button', { name: '30d', exact: true }).click();
  await page.setViewportSize({ width: 1440, height: 1000 });
  await expect(cells.nth(7)).toHaveText('100');
  await expect(cells.nth(8)).toHaveText('$0.0000');
  await expect(component.getByRole('progressbar')).toHaveAttribute('value', '100');
});
