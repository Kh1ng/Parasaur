import { expect, test } from '@playwright/test';

test.afterEach(async ({ page }) => { await page.unrouteAll({ behavior: 'ignoreErrors' }); });

test('Profile saves explicit environment source edits and refreshes redacted configured status', async ({ page, request }) => {
  const mock = process.env.GAH_MOCK_BASE_URL ?? 'http://127.0.0.1:3774';
  await request.post(`${mock}/api/mock/reset`);
  await request.patch(`${mock}/api/profiles/fixture`, { data: { env_file: 'PRIVATE_ENV_SOURCE_CANARY', env_file_prod: '/private/prod.env' } });
  let patch: unknown;
  await page.route('**/api/profiles/*', route => {
    patch = route.request().postDataJSON();
    return route.continue();
  });
  await page.goto('/?page=profile');
  const sources = page.getByRole('region', { name: 'Factory environment files' });
  await expect(sources.getByRole('group', { name: 'Production · configured' })).toBeVisible();
  await sources.getByLabel('Development source', { exact: true }).selectOption('replace');
  await sources.getByLabel('Development file path on node').fill('/srv/project/.env.dev');
  await sources.getByLabel('Production source', { exact: true }).selectOption('clear');
  await sources.getByRole('button', { name: 'Save environment sources' }).click();
  await expect.poll(() => patch).toEqual({ env_file: '/srv/project/.env.dev', clear: ['env_file_prod'] });
  await expect(sources.getByRole('status')).toContainText('Environment sources saved');
  await expect(sources.getByRole('group', { name: 'Production · not configured' })).toBeVisible();
  await expect(page.locator('body')).not.toContainText('PRIVATE_ENV_SOURCE_CANARY');
});
