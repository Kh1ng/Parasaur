import { expect, test } from '@playwright/experimental-ct-react';
import type { ProfileChecks } from '@git-agent-harness/contracts';
import { WorkspaceChecksSection } from '../../src/components/WorkspaceChecksSection.js';

const initial = (): ProfileChecks => ({ profile: 'workspace', validation_commands: ['npm test'], auto_fix_commands: ['npm run format'], validation_timeout_seconds: 300 });

test('checks load only on request and save complete commands without changing the other list', async ({ mount, page }) => {
  let state = initial();
  let reads = 0;
  let patch: unknown;
  await page.route('**/api/profiles/workspace/checks', route => { reads++; return route.fulfill({ json: state }); });
  await page.route('**/api/profiles/workspace', route => {
    patch = route.request().postDataJSON();
    state = { ...state, ...(patch as Partial<ProfileChecks>) };
    return route.fulfill({ json: { success: true } });
  });
  const component = await mount(<WorkspaceChecksSection profile="workspace" />);
  expect(reads).toBe(0);
  await component.getByRole('button', { name: 'Edit factory checks' }).click();
  await expect(component.getByLabel('Validation command 1', { exact: true })).toHaveValue('npm test');
  const command = `node -e "console.log([1,2], 'a,b')"\nprintf done`;
  await component.getByLabel('Validation command 1', { exact: true }).fill(command);
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 1100 });
    await component.screenshot({ path: test.info().outputPath(`workspace-checks-${width}.png`) });
  }
  await component.getByRole('button', { name: 'Save factory checks' }).click();
  expect(patch).toEqual({ validation_commands: [command] });
  await expect(component.getByRole('status')).toHaveText('Checks saved and reloaded.');
  await expect(component.getByLabel('Validation command 1', { exact: true })).toHaveValue(command);
  await expect(component.getByLabel('Automatic fixes command 1', { exact: true })).toHaveValue('npm run format');
});

test('removing every check explicitly clears that list', async ({ mount, page }) => {
  let state = initial();
  let patch: unknown;
  await page.route('**/api/profiles/workspace/checks', route => route.fulfill({ json: state }));
  await page.route('**/api/profiles/workspace', route => {
    patch = route.request().postDataJSON(); state = { ...state, ...(patch as Partial<ProfileChecks>) };
    return route.fulfill({ json: { success: true } });
  });
  const component = await mount(<WorkspaceChecksSection profile="workspace" />);
  await component.getByRole('button', { name: 'Edit factory checks' }).click();
  await component.getByRole('button', { name: 'Remove validation command 1' }).click();
  await component.getByRole('button', { name: 'Save factory checks' }).click();
  expect(patch).toEqual({ validation_commands: [] });
  await expect(component.getByText('No validation commands configured.', { exact: true })).toBeVisible();
});

test('failed writes preserve edits without rendering private CLI output', async ({ mount, page }) => {
  await page.route('**/api/profiles/workspace/checks', route => route.fulfill({ json: initial() }));
  await page.route('**/api/profiles/workspace', route => route.fulfill({ status: 503, json: { message: 'PRIVATE_CLI_OUTPUT_CANARY' } }));
  const component = await mount(<WorkspaceChecksSection profile="workspace" />);
  await component.getByRole('button', { name: 'Edit factory checks' }).click();
  await component.getByLabel('Validation command 1', { exact: true }).fill('cargo test');
  await component.getByRole('button', { name: 'Save factory checks' }).click();
  await expect(component.getByRole('alert')).toContainText('Your edits are kept');
  await expect(component.getByLabel('Validation command 1', { exact: true })).toHaveValue('cargo test');
  await expect(component).not.toContainText('PRIVATE_CLI_OUTPUT_CANARY');
  await expect(component.getByRole('button', { name: 'Save factory checks' })).toBeEnabled();
});

test('a different readback does not silently replace the owner’s edits', async ({ mount, page }) => {
  await page.route('**/api/profiles/workspace/checks', route => route.fulfill({ json: initial() }));
  await page.route('**/api/profiles/workspace', route => route.fulfill({ json: { success: true } }));
  const component = await mount(<WorkspaceChecksSection profile="workspace" />);
  await component.getByRole('button', { name: 'Edit factory checks' }).click();
  await component.getByLabel('Validation command 1', { exact: true }).fill('cargo test');
  await component.getByRole('button', { name: 'Save factory checks' }).click();
  await expect(component.getByRole('alert')).toContainText('node returned different checks');
  await expect(component.getByLabel('Validation command 1', { exact: true })).toHaveValue('cargo test');
  await expect(component.getByRole('status')).toHaveCount(0);
});
