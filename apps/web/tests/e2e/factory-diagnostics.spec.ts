import { expect, test } from '@playwright/test';

test.afterEach(async ({ page }) => { await page.unrouteAll({ behavior: 'ignoreErrors' }); });

test('Overview distinguishes failed factory reads, refreshes status and decisions, and labels retained evidence', async ({ page }, testInfo) => {
  let failing = true;
  let running = true;
  let empty = false;
  let statusReads = 0;
  let decisionReads = 0;
  let mutations = 0;
  await page.route('**/api/loop/status**', route => {
    statusReads++;
    return failing ? route.fulfill({ status: 502, json: { message: 'fixture unavailable' } }) : route.fulfill({ json: { running } });
  });
  await page.route('**/api/loop/last-decision**', route => {
    decisionReads++;
    return failing ? route.fulfill({ status: 502, json: { message: 'fixture unavailable' } }) : route.fulfill({ json: empty ? null : {
      timestamp: new Date().toISOString(), kind: 'wait', reason: 'Review receipt needs attention', work_id: '#42', reason_code: 'review_held'
    } });
  });
  for (const action of ['start', 'stop']) {
    await page.route(`**/api/loop/${action}`, route => { mutations++; return route.fulfill({ json: { started: true, stopped: true } }); });
  }
  await page.goto('/?profile=fixture');
  const main = page.locator('#main-content');
  await expect(main.getByText('Factory status unavailable', { exact: true })).toBeVisible();
  await expect(main.getByRole('alert').filter({ hasText: 'Cannot read factory status' })).toBeVisible();
  await expect(main.getByRole('alert').filter({ hasText: 'Cannot read the last factory decision' })).toBeVisible();
  await expect(main.getByRole('button', { name: 'Turn factory on' })).toBeDisabled();
  await expect(main.getByText('Factory off', { exact: true })).toHaveCount(0);
  await page.screenshot({ path: testInfo.outputPath('factory-diagnostics-failed-desktop.png') });

  const before = { statusReads, decisionReads };
  failing = false;
  await main.getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(main.getByText('Factory on', { exact: true })).toBeVisible();
  await expect(main.getByText(/Review receipt needs attention/)).toBeVisible();
  await expect(main.getByRole('button', { name: 'Turn factory off' })).toBeEnabled();
  expect(statusReads).toBeGreaterThan(before.statusReads);
  expect(decisionReads).toBeGreaterThan(before.decisionReads);

  failing = true;
  await main.getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(main.getByText('Factory status unavailable', { exact: true })).toBeVisible();
  await expect(main.getByText(/Last recorded decision \(refresh failed\)/)).toBeVisible();
  await expect(main.getByRole('button', { name: 'Turn factory on' })).toBeDisabled();

  failing = false;
  running = false;
  empty = true;
  await main.getByRole('button', { name: 'Refresh', exact: true }).click();
  await expect(main.getByText('Factory off', { exact: true })).toBeVisible();
  await expect(main.getByText('No factory decision recorded in the last 24 hours.')).toBeVisible();
  await expect(main.getByText(/Review receipt needs attention/)).toHaveCount(0);
  await expect(main.getByRole('button', { name: 'Turn factory on' })).toBeEnabled();
  expect(mutations).toBe(0);
  await page.setViewportSize({ width: 390, height: 844 });
  for (const name of ['Turn factory on', 'Refresh']) {
    const box = await main.getByRole('button', { name, exact: true }).boundingBox();
    expect(box!.height).toBeGreaterThanOrEqual(44);
    expect(box!.width).toBeGreaterThanOrEqual(44);
  }
  await page.screenshot({ path: testInfo.outputPath('factory-diagnostics-recovered-mobile.png') });
  expect(await main.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
});
