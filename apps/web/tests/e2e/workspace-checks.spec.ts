import { expect, test } from '@playwright/test';

test('Profile edits and clears complete command lists through the sample API', async ({ page, request }) => {
  const mock = process.env.GAH_MOCK_BASE_URL ?? 'http://127.0.0.1:3774';
  await request.post(`${mock}/api/mock/reset`);
  await request.patch(`${mock}/api/profiles/fixture`, { data: { validation_commands: ['npm test'], auto_fix_commands: ['npm run format'] } });
  await page.goto('/?page=profile');
  const checks = page.getByRole('region', { name: 'Factory checks', exact: true });
  await checks.getByRole('button', { name: 'Edit factory checks' }).click();
  await expect(checks.getByLabel('Validation command 1', { exact: true })).toHaveValue('npm test');
  const command = `node -e "console.log([1,2], 'a,b')"\nprintf done`;
  await checks.getByLabel('Validation command 1', { exact: true }).fill(command);
  await checks.getByRole('button', { name: 'Save factory checks' }).click();
  await expect(checks.getByRole('status')).toHaveText('Checks saved and reloaded.');
  const saved = await (await request.get(`${mock}/api/profiles/fixture/checks`)).json();
  expect(saved.validation_commands).toEqual([command]);
  expect(saved.auto_fix_commands).toEqual(['npm run format']);
  await checks.getByRole('button', { name: 'Remove validation command 1' }).click();
  await checks.getByRole('button', { name: 'Save factory checks' }).click();
  await expect(checks.getByRole('status')).toHaveText('Checks saved and reloaded.');
  const cleared = await (await request.get(`${mock}/api/profiles/fixture/checks`)).json();
  expect(cleared.validation_commands).toEqual([]);
  expect(cleared.auto_fix_commands).toEqual(['npm run format']);
});
