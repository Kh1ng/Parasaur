import { expect, test } from '@playwright/experimental-ct-react';
import { SubscriptionCards } from '../../src/components/SubscriptionCards.js';
import { SubscriptionUsageMenu } from '../../src/components/SubscriptionUsageMenu.js';
import { subscriptionUsage } from '../../src/lib/subscriptionUsage.js';

test('account cards and navbar distinguish recorded actual, estimated, missing and zero costs', async ({ mount, page }, testInfo) => {
  const subscriptions = subscriptionUsage({ since: '7d', candidates: ['work', 'personal', 'legacy'].map(instance => ({
    backend: 'codex', backend_instance: instance, provider: 'openai', model: 'a', eligible_now: true, modes: [], usage: { actual_cost_usd: 99 },
  })), account_spend: [
    { backend: 'codex', backend_instance: 'work', observations: 2, actual_cost_usd: 2, estimated_cost_usd: 3, actual_cost_unknown_observations: 1 },
    { backend: 'codex', backend_instance: 'personal', observations: 1, actual_cost_usd: 0, estimated_cost_usd: null, actual_cost_unknown_observations: 0 },
  ] } as never);
  const component = await mount(<div><SubscriptionCards subscriptions={subscriptions} /><div className="flex justify-end"><SubscriptionUsageMenu subscriptions={subscriptions} onOpenQuota={() => undefined} /></div></div>);
  for (const card of await component.getByRole('article').all()) await card.getByRole('button').click();
  const work = component.getByRole('article').filter({ hasText: /work/i });
  await expect(work).toContainText('Recorded actual cost (7d): $2.00 · Estimated cost: $3.00 · Some recorded costs are unknown.');
  await expect(component.getByRole('article').filter({ hasText: /personal/i })).toContainText('Recorded actual cost (7d): $0.0000');
  await expect(component.getByRole('article').filter({ hasText: /legacy/i })).toContainText('Recorded actual cost (7d): Unknown');
  await expect(component).not.toContainText('$99');
  await component.getByRole('button', { name: 'OpenAI work usage', exact: true }).click();
  await expect(component.getByRole('dialog')).toContainText('Recorded actual cost (7d): $2.00 · Estimated cost: $3.00');
  await expect(component.getByRole('dialog')).toContainText('provider billing may differ');
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 900 });
    expect(await component.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
    await page.screenshot({ path: testInfo.outputPath(`account-cost-${width}.png`), fullPage: true });
  }
});


test('expired provider windows are historical in the navbar and collapsed and expanded cards', async ({ mount, page }) => {
  await page.clock.install({ time: new Date('2026-11-05T20:00:00Z') });
  const subscriptions = subscriptionUsage({ candidates: [{
    backend: 'opencode', backend_instance: 'opencode', provider: 'mistral', eligible_now: true, modes: [],
    quota_observations: [{ quota_window: 'monthly', quota_remaining_percent: 0, quota_reset_at: '2026-10-25T12:04:00Z', observed_at: '2026-10-24T12:00:00Z' }],
  }] } as never);
  const component = await mount(<div><SubscriptionCards subscriptions={subscriptions} /><SubscriptionUsageMenu subscriptions={subscriptions} onOpenQuota={() => undefined} /></div>);
  const card = component.getByRole('article');
  await expect(card).toContainText('last recorded 100%');
  await card.getByRole('button').click();
  await expect(card).toContainText('Current usage unavailable');
  await expect(card).toContainText('Last recorded: 100%');
  await component.getByRole('button', { name: /Mistral opencode usage unavailable; last recorded/ }).click();
  const dialog = component.getByRole('dialog');
  await expect(dialog).toContainText('Current usage unavailable');
  await expect(dialog).toContainText('Recorded reset');
  await expect(component).not.toContainText('Resets in');
  await expect(component).not.toContainText('Resets now');
  await expect(component).not.toContainText('tightest limit right now');
});

test('a mounted navbar expires a window without requiring hover or a new snapshot', async ({ mount, page }) => {
  await page.clock.install({ time: new Date('2026-11-05T20:00:00Z') });
  const subscriptions = subscriptionUsage({ candidates: [{
    backend: 'opencode', backend_instance: 'opencode', provider: 'mistral', eligible_now: true, modes: [],
    quota_observations: [{ quota_window: 'daily', quota_remaining_percent: 75, quota_reset_at: '2026-11-05T20:00:30Z' }],
  }] } as never);
  const component = await mount(<SubscriptionUsageMenu subscriptions={subscriptions} onOpenQuota={() => undefined} />);
  await expect(component.getByRole('button', { name: 'Mistral opencode usage: 25% of daily used', exact: true })).toBeVisible();
  await page.clock.runFor(30_000);
  await expect(component.getByRole('button', { name: /usage unavailable; last recorded/ })).toBeVisible();
});
