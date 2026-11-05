import { expect, test } from '@playwright/test';

test('quota failures and pending retries keep recorded costs without claiming current allowance', async ({ page }, testInfo) => {
  let failing = false;
  let held = false;
  let recovered = false;
  let release: (() => void) | undefined;
  await page.route('**/api/quota?*', async route => {
    if (failing) return route.fulfill({ status: 502, json: { message: 'quota unavailable' } });
    if (held) await new Promise<void>(resolve => { release = resolve; });
    const snapshot = await (await route.fetch()).json();
    const now = new Date().toISOString();
    snapshot.quota_checks = [];
    snapshot.candidates = [{ backend: 'codex', backend_instance: 'work', provider: 'openai', model: 'fixture-model', modes: ['improve'], configured: true, eligible_now: true, observed_at: now, usage: snapshot.usage,
      quota_observations: [{ backend: 'codex', quota_window: '5-hour', quota_remaining_percent: recovered ? 40 : 70, observed_at: now, quota_reset_at: new Date(Date.now() + 3_600_000).toISOString() }] }];
    snapshot.account_spend = [{ backend: 'codex', backend_instance: 'work', observations: 1, actual_cost_usd: 2, estimated_cost_usd: null, actual_cost_unknown_observations: 0 }];
    await route.fulfill({ json: snapshot });
  });
  try {
    await page.goto('/?page=quota&profile=fixture');
    const main = page.locator('#main-content');
    const refresh = main.getByRole('button', { name: 'Refresh', exact: true });
    const row = main.getByTestId('quota-candidate-codex-0');
    await expect(row.getByText('Eligible', { exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: /^OpenAI work usage: 30%/ })).toBeVisible();
    await main.getByRole('region', { name: 'Subscriptions', exact: true }).getByRole('button', { name: /OpenAI/ }).click();
    await expect(main).toContainText('Recorded actual cost (7d): $2.00');
    failing = true;
    await refresh.click();
    const warning = main.getByRole('alert').filter({ hasText: 'Quota snapshot unavailable' });
    await expect(warning).toContainText('Current allowances and route eligibility are unconfirmed.');
    await expect(row.getByText('Eligibility unconfirmed', { exact: true })).toBeVisible();
    await expect(row.getByText('Eligible', { exact: true })).toHaveCount(0);
    await expect(main).not.toContainText('1 / 1 not blocked');
    await page.getByRole('button', { name: /^OpenAI work usage unavailable; last recorded: 30%/ }).click();
    await expect(page.getByRole('dialog', { name: 'OpenAI usage' })).toContainText('Current usage unavailable. These are last recorded readings.');
    await page.keyboard.press('Escape');
    for (const width of [1100, 390]) {
      await page.setViewportSize({ width, height: 844 });
      expect(await warning.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
      await page.screenshot({ path: testInfo.outputPath(`quota-unavailable-${width}.png`), fullPage: true });
    }
    failing = false;
    held = true;
    await warning.getByRole('button', { name: 'Retry quota', exact: true }).click();
    await expect(refresh).toBeDisabled();
    await expect(warning).toBeVisible();
    await expect(row.getByText('Eligibility unconfirmed', { exact: true })).toBeVisible();
    await expect.poll(() => Boolean(release)).toBe(true);
    recovered = true;
    held = false;
    release!();
    await expect(warning).toHaveCount(0);
    await expect(row.getByText('Eligible', { exact: true })).toBeVisible();
    await expect(page.getByRole('button', { name: /^OpenAI work usage: 60%/ })).toBeVisible();
    await expect(main).toContainText('Recorded actual cost (7d): $2.00');
  } finally { release?.(); await page.unrouteAll({ behavior: 'ignoreErrors' }); }
});
