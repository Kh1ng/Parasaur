import { expect, test } from '@playwright/test';

test('the fixture server identifies sample data across page navigation', async ({ page }) => {
  await page.goto('/');
  const sample = page.getByRole('complementary', { name: 'Sample data environment' });
  await expect(sample).toContainText('No live factory is connected');
  await expect(sample).toContainText('update notices are simulated');
  await page.getByRole('button', { name: 'Factory', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Factory', exact: true })).toBeVisible();
  await expect(sample).toBeVisible();
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await expect(sample).toBeVisible();
    expect(await sample.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
    await page.screenshot({ path: test.info().outputPath(`sample-environment-${width}.png`) });
  }
});

test('the dashboard does not infer sample mode from a missing identity', async ({ page }) => {
  await page.route('**/api/info', route => route.fulfill({ status: 503, json: { message: 'Unavailable' } }));
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Overview', exact: true })).toBeVisible();
  await expect(page.getByRole('complementary', { name: 'Sample data environment' })).toHaveCount(0);
});

test('review receipt fields explain the same paused handoff in work details and Kanban', async ({ page }) => {
  await page.route('**/api/events**', route => route.fulfill({ json: [
    { profile: 'fixture', work_id: 'TICKET-000116', timestamp: '2026-11-02T12:00:00Z', event_type: 'dispatch_finished',
      details: 'No configured route could take this job.', reason_code: 'deferred_capacity',
      skipped: [{ backend: 'codex', backend_instance: 'codex-work', model: 'model-one', reason: 'max_concurrent_reached' }] },
    { profile: 'another-project', work_id: '#116', timestamp: '2026-11-02T13:00:00Z', event_type: 'dispatch_finished', details: 'Other project must not leak.' },
  ] }));
  await page.route('**/api/status**', async route => {
    const snapshot = await (await route.fetch()).json();
    snapshot.merge_requests = [{
      work_id: '#116', branch: 'gah/116', id: '42', url: 'https://github.com/example/repo/pull/42',
      title: 'Review receipt handoff', state: 'OPEN', draft: true, merged: false,
      classification: 'NEEDS_REVIEW', recommended_action: 'RUN_REVIEW', ci_passed: true, ci_pending: false,
      review_contract_version: 1, source_sha: 'head-one', last_reviewed_head: 'head-one', review_verdict: 'APPROVE',
    }];
    await route.fulfill({ json: snapshot });
  });
  await page.goto('/?page=work');
  await page.getByRole('row').filter({ hasText: '#116' }).getByRole('button', { name: 'View details' }).click();
  await expect(page.getByRole('dialog')).toContainText('Automatic review is paused until a new commit is pushed');
  const decisions = page.getByRole('region', { name: 'Recent controller decisions' });
  await expect(decisions).toContainText('No configured route could take this job.');
  await expect(decisions).toContainText('codex-work / model-one: max_concurrent_reached');
  await expect(decisions).not.toContainText('Other project must not leak');
  await page.getByRole('button', { name: 'Close work details' }).click();
  await page.getByRole('button', { name: 'Kanban', exact: true }).click();
  await expect(page.getByText('This commit has already been reviewed.', { exact: false }).first()).toBeVisible();
});
