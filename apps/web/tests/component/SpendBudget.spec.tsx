import { expect, test } from '@playwright/experimental-ct-react';
import React from 'react';
import type { StatusSnapshot } from '@git-agent-harness/contracts';
import { SpendBudgetCard } from '../../src/components/SpendBudgetCard.js';

const budget: NonNullable<StatusSnapshot['spend_budget']> = {
  cost_basis: 'actual', scope: 'recorded-factory-ledger', intake_held: true,
  hold_reason: 'Daily cost coverage is incomplete; intake paused until readings can be verified.',
  retry_at: '2030-01-31T12:01:00Z',
  windows: [{ window: 'day', starts_at: '2030-01-31T00:00:00Z', resets_at: '2030-02-01T00:00:00Z',
    limit_usd: 10, reported_usd: null, measured_observations: 0, unmeasured_observations: 3, exhausted: false, incomplete: true },
  { window: 'month', starts_at: '2030-01-01T00:00:00Z', resets_at: '2030-02-01T00:00:00Z',
    limit_usd: 100, reported_usd: 4.5, measured_observations: 2, unmeasured_observations: 3, exhausted: false, incomplete: true }],
};

test('spend status retains unknown costs, window scope and readable narrow layout', async ({ mount, page }, testInfo) => {
  const component = await mount(<SpendBudgetCard budget={budget} />);
  await expect(component.getByText('Intake paused', { exact: true })).toBeVisible();
  await expect(component.getByText('Cost not reported / $10.00 limit', { exact: true })).toBeVisible();
  await expect(component.getByText('$4.50 recorded / $100.00 limit', { exact: true })).toBeVisible();
  await expect(component.getByText('3 cost readings missing', { exact: true })).toHaveCount(2);
  await expect(component.getByText('Based on reported actual cost in this node’s factory ledger.')).toBeVisible();
  await page.setViewportSize({ width: 1100, height: 500 });
  await page.screenshot({ path: testInfo.outputPath('spend-budget-desktop.png') });
  await page.setViewportSize({ width: 390, height: 650 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('spend-budget-narrow.png') });
});

test('an estimate budget says estimates and absent policy shows no invented limit', async ({ mount }) => {
  const component = await mount(<SpendBudgetCard budget={{ ...budget, cost_basis: 'estimated', intake_held: false, hold_reason: null,
    windows: [{ ...budget.windows[0], reported_usd: 0, unmeasured_observations: 0, incomplete: false }] }} />);
  await expect(component.getByText('Within recorded ceiling', { exact: true })).toBeVisible();
  await expect(component.getByText('$0.0000 recorded / $10.00 limit', { exact: true })).toBeVisible();
  await expect(component.getByText('Based on API-equivalent estimates in this node’s factory ledger.')).toBeVisible();
  await component.update(<SpendBudgetCard budget={undefined} />);
  await expect(component.getByRole('heading')).toHaveCount(0);
});

