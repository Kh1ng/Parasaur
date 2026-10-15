import { test, expect } from '@playwright/experimental-ct-react';
import { LoginRepairPanel } from '../../src/components/LoginRepairPanel.js';

test('a named Claude login shows instance repair instructions without ambient auth', async ({ mount, page }) => {
  let started = 0;
  await page.route('**/api/auth-health/repairs', route => { started++; return route.abort(); });
  const component = await mount(<LoginRepairPanel login={{ node_id: 'worker', node_name: 'Work Mac',
    backend: 'claude', backend_instance: 'claude-work', provider: null }} />);
  await expect(component.getByText('Repair instance claude-work on Work Mac.', { exact: false })).toBeVisible();
  await expect(component.getByText(/replace its saved credential/)).toBeVisible();
  await expect(component.getByRole('button', { name: 'Fix login' })).toHaveCount(0);
  expect(started).toBe(0);
});

for (const [backend, provider, label, url] of [
  ['gh', 'github', 'GitHub CLI', 'https://cli.github.com/'],
  ['glab', 'gitlab', 'GitLab CLI', 'https://gitlab.com/gitlab-org/cli#installation'],
]) {
  test(`${label} installation precedes authentication`, async ({ mount, page }) => {
    await page.route('**/api/auth-health/repairs', route => route.fulfill({
      json: { key: 'test-key', repair: { id: 'repair-1', node_id: 'central', backend, provider,
        expires_at: new Date(Date.now() + 60_000).toISOString(), status: 'install_required', install_url: url } },
    }));
    const component = await mount(<LoginRepairPanel login={{ node_id: 'central', node_name: 'This computer', backend, provider, installed: false }} />);
    await expect(component.getByRole('link', { name: `Install ${label}` })).toHaveAttribute('href', url);
    await expect(component.getByRole('button', { name: 'Fix login' })).toHaveCount(0);
    await component.getByRole('button', { name: 'Check and sign in' }).click();
    await expect(component.getByText('Sign-in starts only after the CLI is available.', { exact: false })).toBeVisible();
    await expect(component.getByRole('link', { name: 'Open sign-in page' })).toHaveCount(0);
  });
}
