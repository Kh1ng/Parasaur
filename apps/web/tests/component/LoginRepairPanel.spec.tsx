import { expect, test } from '@playwright/experimental-ct-react';
import { LoginRepairPanel } from '../../src/components/LoginRepairPanel.js';

test('login status retries repeated read failures, clears the error, and stops on completion', async ({ mount, page }) => {
  await page.clock.install();
  const repair = { id: 'repair-1', node_id: 'worker', backend: 'codex', provider: null,
    expires_at: new Date(Date.now() + 600_000).toISOString(), status: 'waiting' };
  await page.route('**/api/auth-health/repairs', route => route.fulfill({ json: { key: 'test-key', repair } }));
  let reads = 0;
  await page.route('**/api/auth-health/repairs/repair-1', route => {
    reads++;
    expect(route.request().headers()['x-login-repair-key']).toBe('test-key');
    return reads <= 2
      ? route.fulfill({ status: 503, json: { message: 'Node temporarily unavailable' } })
      : route.fulfill({ json: { ...repair, status: 'succeeded' } });
  });
  const component = await mount(<LoginRepairPanel login={{ node_id: 'worker', node_name: 'Work PC', backend: 'codex', provider: null }} />);
  await component.getByRole('button', { name: 'Fix login' }).click();
  await expect(component.getByRole('status')).toHaveText('Waiting for codex on Work PC…');
  for (const count of [1, 2]) {
    await page.clock.runFor(2_000);
    await expect.poll(() => reads).toBe(count);
    await expect(component.getByRole('alert')).toHaveText('Node temporarily unavailable');
  }
  await page.clock.runFor(2_000);
  await expect(component.getByRole('status')).toHaveText('Logged in again. Work PC re-checked the login.');
  await expect(component.getByRole('alert')).toHaveCount(0);
  await page.clock.runFor(10_000);
  expect(reads).toBe(3);
});

test('unmounting after a failed read stops background login polling', async ({ mount, page }) => {
  await page.clock.install();
  await page.route('**/api/auth-health/repairs', route => route.fulfill({ json: { key: 'test-key', repair: {
    id: 'repair-2', node_id: 'worker', backend: 'codex', provider: null, status: 'waiting', expires_at: '' } } }));
  let reads = 0;
  await page.route('**/api/auth-health/repairs/repair-2', route => {
    reads++;
    return route.fulfill({ status: 503, json: { message: 'Node temporarily unavailable' } });
  });
  const component = await mount(<LoginRepairPanel login={{ node_id: 'worker', backend: 'codex', provider: null }} />);
  await component.getByRole('button', { name: 'Fix login' }).click();
  await page.clock.runFor(2_000);
  await expect(component.getByRole('alert')).toBeVisible();
  await component.unmount();
  await page.clock.runFor(10_000);
  expect(reads).toBe(1);
});
