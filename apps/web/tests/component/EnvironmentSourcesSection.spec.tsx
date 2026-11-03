import { expect, test } from '@playwright/experimental-ct-react';
import { EnvironmentSourcesSection } from '../../src/components/EnvironmentSourcesSection.js';

test('environment sources preserve untouched settings and send explicit paths and clear operations', async ({ mount, page }) => {
  let body: unknown;
  await page.route('**/api/profiles/workspace', route => {
    body = route.request().postDataJSON();
    return route.fulfill({ json: { success: true } });
  });
  const component = await mount(<EnvironmentSourcesSection profile="workspace" configured={{ env_file: true, env_file_prod: true }} onSaved={async () => {}} />);
  await expect(component.getByRole('button', { name: 'Save environment sources' })).toBeDisabled();
  await expect(component.getByRole('textbox')).toHaveCount(0);
  await component.getByLabel('Development source', { exact: true }).selectOption('replace');
  await component.getByLabel('Development file path on node').fill('/srv/workspace/.env.dev');
  await component.getByLabel('Production source', { exact: true }).selectOption('clear');
  for (const width of [960, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await component.screenshot({ path: test.info().outputPath(`environment-sources-${width}.png`) });
  }
  await component.getByRole('button', { name: 'Save environment sources' }).click();
  expect(body).toEqual({ env_file: '/srv/workspace/.env.dev', clear: ['env_file_prod'] });
  await expect(component.getByRole('status')).toHaveText('Environment sources saved for new factory dispatches.');
  await expect(component.getByRole('textbox')).toHaveCount(0);
});

test('a failed save retains the path without echoing private node error text', async ({ mount, page }) => {
  await page.route('**/api/profiles/workspace', route => route.fulfill({ status: 503, json: { message: 'PRIVATE_ENV_SOURCE_CANARY' } }));
  const component = await mount(<EnvironmentSourcesSection profile="workspace" configured={{ env_file: false, env_file_prod: false }} onSaved={async () => {}} />);
  await component.getByLabel('Production source', { exact: true }).selectOption('replace');
  await expect(component.getByRole('button', { name: 'Save environment sources' })).toBeDisabled();
  await component.getByLabel('Production file path on node').fill('/srv/workspace/production.env');
  await component.getByRole('button', { name: 'Save environment sources' }).click();
  await expect(component.getByRole('alert')).toContainText('Your entries are kept');
  await expect(component.getByLabel('Production file path on node')).toHaveValue('/srv/workspace/production.env');
  await expect(component).not.toContainText('PRIVATE_ENV_SOURCE_CANARY');
  await expect(component.getByRole('button', { name: 'Save environment sources' })).toBeEnabled();
});
