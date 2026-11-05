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
