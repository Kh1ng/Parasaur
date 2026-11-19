import { expect, test } from '@playwright/test';

test('Nodes reads source-scoped factory evidence through the stateful sample endpoint', async ({ page }) => {
  const reads: string[] = [];
  page.on('request', request => { if (new URL(request.url()).pathname.endsWith('/factory')) reads.push(request.method()); });
  await page.goto('/?page=nodes');
  await expect(page.getByRole('complementary', { name: 'Sample data environment' })).toBeVisible();
  await page.getByRole('button', { name: 'Mock worker', exact: true }).click();
  const panel = page.getByRole('region', { name: 'Worker factory state' });
  await panel.getByRole('button', { name: 'Read factory state', exact: true }).click();
  await expect(panel).toContainText('Controller reported stopped.');
  await expect(panel).toContainText('Sample review gate awaiting operator action.');
  await expect(panel).toContainText('Sample worker: review repeated without progress');
  await expect(panel).toContainText('gah · mock-worker');
  expect(reads).toEqual(['GET']);
});
