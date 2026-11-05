import { expect, test } from '@playwright/test';

test('failed and pending status refreshes retain context without claiming live execution', async ({ page }, testInfo) => {
  let failing = false;
  let holdRetry = false;
  let releaseRetry: (() => void) | undefined;
  let assignments = 0;
  page.on('websocket', socket => socket.on('framesent', event => {
    const message = JSON.parse(event.payload.toString());
    if (message.type === 'session.start') assignments++;
  }));
  await page.route('**/api/status**', async route => {
    if (failing) return route.fulfill({ status: 502, json: { message: 'fixture node unavailable' } });
    if (holdRetry) await new Promise<void>(resolve => { releaseRetry = resolve; });
    const snapshot = await (await route.fetch()).json();
    snapshot.available_tickets = [{ work_id: '#117', ticket_path: '117', title: 'Operator decision', prior_attempt_count: 0, human_required: true }];
    snapshot.merge_requests = [];
    snapshot.active_claims = [];
    snapshot.blocked_work_items = [];
    snapshot.running_workers = [{ work_id: '#116', run_id: 'fixture-run', mode: 'improve', backend: 'codex', runner: 'codex', backend_instance: 'codex-work', requested_model: 'fixture-model', model: 'fixture-model', actual_model: null, node_id: 'fixture-node', branch: 'gah/116', started_at: new Date().toISOString(), last_activity_at: new Date().toISOString(), attempt: 1, stale_after_seconds: 900, state: 'running' }];
    await route.fulfill({ json: snapshot });
  });
  await page.goto('/?page=kanban&profile=fixture');
  const main = page.locator('#main-content');
  const refresh = main.getByRole('button', { name: 'Refresh', exact: true });
  await expect(main.getByRole('region', { name: /^Running, / })).toContainText('#116');
  await expect(page.getByRole('button', { name: /^Working on #116/ })).toBeVisible();

  failing = true;
  await refresh.click();
  const warning = main.getByRole('alert').filter({ hasText: 'Factory state could not refresh' });
  await expect(warning).toContainText('last recorded observations');
  await expect(main.getByRole('region', { name: /^Waiting, / })).toContainText('#116');
  await expect(main.getByRole('img', { name: 'Running now' })).toHaveCount(0);
  await expect(main.getByRole('button', { name: 'Assign', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: 'Factory activity unavailable', exact: true }).click();
  const menu = page.getByRole('dialog', { name: 'What the factory is doing now' });
  await expect(menu).toContainText('Last recorded jobs');
  await expect(menu).toContainText('#116');
  await expect(menu).not.toContainText('Nothing is running.');
  await page.keyboard.press('Escape');
  await page.screenshot({ path: testInfo.outputPath('failed-refresh-desktop.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(warning).toBeVisible();
  expect(await warning.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('failed-refresh-mobile.png') });

  failing = false;
  holdRetry = true;
  await refresh.click();
  await expect(refresh).toBeDisabled();
  await expect(warning).toBeVisible();
  await expect(main.getByRole('img', { name: 'Running now' })).toHaveCount(0);
  await expect.poll(() => Boolean(releaseRetry)).toBe(true);
  holdRetry = false;
  releaseRetry!();
  await expect(warning).toHaveCount(0);
  await expect(main.getByRole('region', { name: /^Running, / })).toContainText('#116');
  expect(assignments).toBe(0);
});
