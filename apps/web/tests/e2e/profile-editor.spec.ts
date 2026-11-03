import { expect, test } from '@playwright/test';

test.afterEach(async ({ page }) => { await page.unrouteAll({ behavior: 'ignoreErrors' }); });

async function openEditor(page: import('@playwright/test').Page) {
  await page.route('**/api/profiles', async route => {
    if (route.request().method() !== 'GET') return route.fallback();
    const response = await route.fetch();
    const profiles = await response.json();
    profiles[0] = { ...profiles[0], artifact_root: '/srv/retained-artifacts', default_target_branch: 'release/stable' };
    await route.fulfill({ json: profiles });
  });
  await page.goto('/?page=profile');
  await page.getByRole('button', { name: 'Edit profile', exact: true }).first().click();
}

test('editing a name preserves loaded workspace settings and only patches edited fields', async ({ page }) => {
  let patch: unknown;
  await page.route('**/api/profiles/*', route => {
    patch = route.request().postDataJSON();
    return route.fulfill({ json: { success: true } });
  });
  await openEditor(page);
  await expect(page.getByLabel('Artifact Root', { exact: true })).toHaveValue('/srv/retained-artifacts');
  await expect(page.getByLabel('Default Branch', { exact: true })).toHaveValue('release/stable');
  await page.getByLabel('Display Name *', { exact: true }).fill('Renamed workspace');
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 1000 });
    const form = page.locator('form').filter({ has: page.getByLabel('Display Name *', { exact: true }) });
    await form.screenshot({ path: test.info().outputPath(`profile-editor-${width}.png`) });
  }
  await page.getByRole('button', { name: 'Update Profile', exact: true }).click();
  await expect.poll(() => patch).toEqual({ display_name: 'Renamed workspace' });
  await expect(page.getByLabel('Display Name *', { exact: true })).toHaveCount(0);
});

test('failed edits keep the form and typed workspace changes available for retry', async ({ page }) => {
  await page.route('**/api/profiles/*', route => route.fulfill({ status: 503, json: { message: 'Node unavailable' } }));
  await openEditor(page);
  await page.getByLabel('Default Branch', { exact: true }).fill('next-release');
  await page.getByRole('button', { name: 'Update Profile', exact: true }).click();
  await expect(page.getByRole('alert').filter({ hasText: /^Node unavailable$/ })).toBeVisible();
  await expect(page.getByLabel('Default Branch', { exact: true })).toHaveValue('next-release');
  await expect(page.getByRole('button', { name: 'Update Profile', exact: true })).toBeEnabled();
});

test('an edited branch must be nonempty and can be saved without replacing other settings', async ({ page }) => {
  let patch: unknown;
  await page.route('**/api/profiles/*', route => { patch = route.request().postDataJSON(); return route.fulfill({ json: { success: true } }); });
  await openEditor(page);
  const branch = page.getByLabel('Default Branch', { exact: true });
  await branch.fill('');
  await page.getByRole('button', { name: 'Update Profile', exact: true }).click();
  expect(await branch.evaluate(input => (input as HTMLInputElement).validity.valueMissing)).toBe(true);
  expect(patch).toBeUndefined();
  await branch.fill('develop');
  await page.getByRole('button', { name: 'Update Profile', exact: true }).click();
  await expect.poll(() => patch).toEqual({ default_target_branch: 'develop' });
});

test('failed deletes stay open and explain the failure inside the confirmation', async ({ page }) => {
  await page.route('**/api/profiles/*', route => route.fulfill({ status: 503, json: { message: 'Delete unavailable' } }));
  await page.goto('/?page=profile');
  await page.getByRole('button', { name: 'Delete profile', exact: true }).first().click();
  const dialog = page.getByRole('dialog', { name: 'Delete Profile', exact: true });
  await dialog.getByRole('button', { name: 'Delete Profile', exact: true }).click();
  await expect(dialog.getByRole('alert')).toHaveText('Delete unavailable');
  await expect(dialog.getByRole('button', { name: 'Cancel', exact: true })).toBeEnabled();
});

test('new profiles use the explicitly entered ID and retain input on failure', async ({ page }) => {
  let body: { name?: string } | undefined;
  await page.route('**/api/profiles', async route => {
    if (route.request().method() !== 'POST') return route.fallback();
    body = route.request().postDataJSON();
    await route.fulfill({ status: 503, json: { message: 'Create unavailable' } });
  });
  await page.goto('/?page=profile');
  await page.getByRole('button', { name: 'Add Profile', exact: true }).click();
  for (const [label, value] of [['Display Name *', 'Team workspace'], ['Profile Name (ID) *', 'explicit-id'],
    ['Repository *', 'owner/repo'], ['Local Path *', '/srv/repo'], ['Artifact Root *', '/srv/artifacts']]) {
    await page.getByLabel(label, { exact: true }).fill(value);
  }
  await page.getByRole('button', { name: 'Add Profile', exact: true }).last().click();
  await expect.poll(() => body?.name).toBe('explicit-id');
  await expect(page.getByRole('alert')).toContainText('Create unavailable');
  await expect(page.getByLabel('Profile Name (ID) *', { exact: true })).toHaveValue('explicit-id');
});
