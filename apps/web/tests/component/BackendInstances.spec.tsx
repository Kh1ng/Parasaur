import { expect, test } from '@playwright/experimental-ct-react';
import { BackendInstancesCard } from '../../src/pages/ProfilePanel.js';
import type { BackendInstanceSummary } from '@git-agent-harness/contracts';
import { LoginRepairPanel } from '../../src/components/LoginRepairPanel.js';

test('connected-node instance creation supports all runners without exposing local keys', async ({ mount, page }) => {
  let added: unknown;
  await page.route('**/api/backend-instances**', route => {
    if (route.request().method() === 'POST') added = route.request().postDataJSON();
    return route.fulfill({ json: { profile: 'central-profile', backend_instances: [] } });
  });
  const component = await mount(<BackendInstancesCard profileName="central-profile" effective={{ backend_instances: [] }} />);
  await expect(component.getByText(/on the connected node/)).toBeVisible();
  await component.getByRole('button', { name: 'Add account', exact: true }).click();
  await expect(component.getByLabel('Runner').getByRole('option')).toHaveText(['Codex', 'Claude', 'OpenCode', 'Vibe', 'OpenHands', 'Hermes', 'Antigravity']);
  await component.getByLabel('Runner').selectOption('vibe');
  await component.getByLabel('Account label').fill('Vibe second');
  await component.getByLabel('Instance ID').fill('vibe-second');
  await component.getByRole('button', { name: 'Add isolated account' }).click();
  await expect(component.getByRole('button', { name: 'Add account', exact: true })).toBeVisible();
  expect(added).toEqual({ profile: 'central-profile', instance: 'vibe-second', runnerKind: 'vibe', accountLabel: 'Vibe second' });
  await expect(component.locator('input[type="password"]')).toHaveCount(0);
});

test('closing while a named login is starting cancels the repair when its key arrives', async ({ mount, page }) => {
  let release!: () => void;
  const response = new Promise<void>(resolve => { release = resolve; });
  let starting = false;
  let cancelled = 0;
  await page.route('**/api/backend-instances/authenticate', async route => {
    starting = true;
    await response;
    await route.fulfill({ status: 201, json: { key: 'late-key', repair: {
      id: 'late-repair', node_id: 'central', backend: 'codex', provider: null, status: 'waiting', expires_at: '' } } });
  });
  await page.route('**/api/auth-health/repairs/late-repair', route => {
    expect(route.request().method()).toBe('DELETE');
    cancelled++;
    return route.fulfill({ json: { cancelled: true } });
  });
  const component = await mount(<LoginRepairPanel instanceProfile="alpha" login={{ node_id: '', backend: 'codex', backend_instance: 'work', provider: null }} />);
  await component.getByRole('button', { name: 'Start sign-in' }).click();
  await expect.poll(() => starting).toBe(true);
  await component.unmount();
  release();
  await expect.poll(() => cancelled).toBe(1);
});

test('an older node explains that account sign-in requires an update', async ({ mount, page }) => {
  await page.route('**/api/backend-instances/authenticate', route => route.fulfill({ status: 404 }));
  const component = await mount(<LoginRepairPanel instanceProfile="alpha" login={{ node_id: '', backend: 'codex', backend_instance: 'work', provider: null }} />);
  await component.getByRole('button', { name: 'Start sign-in' }).click();
  await expect(component.getByRole('alert')).toContainText('Update Parasaur on the connected node');
});

test('an isolated named account signs in through the connected node and refreshes its own auth status', async ({ mount, page }) => {
  await page.clock.install();
  let authenticated = false;
  let started: unknown;
  const account = { backend_instance: 'codex-work', runner_kind: 'codex', logical_backend: 'codex', enabled: true,
    isolated_state_configured: true, executable_configured: true, account_label: 'Work', auth_ready: false, supported_models: [], auth_source_label: null, quota_pool: null } satisfies BackendInstanceSummary;
  const repair = { id: 'named-repair', node_id: 'central', backend: 'codex', provider: null,
    expires_at: new Date(Date.now() + 600_000).toISOString(), status: 'open_url', url: 'https://auth.openai.com/codex/device', code: 'K7QD-M2XPA' };
  await page.route('**/api/backend-instances**', route => {
    if (route.request().method() === 'POST') {
      started = route.request().postDataJSON();
      expect(route.request().headers()['idempotency-key']).toBeTruthy();
      return route.fulfill({ status: 201, json: { key: 'named-key', repair } });
    }
    return route.fulfill({ json: { profile: 'central-profile', backend_instances: [{ ...account, auth_ready: authenticated }] } });
  });
  await page.route('**/api/auth-health/repairs/named-repair', route => {
    authenticated = true;
    return route.fulfill({ json: { ...repair, status: 'succeeded' } });
  });
  const component = await mount(<BackendInstancesCard profileName="central-profile" effective={{ backend_instances: [account] }} />);
  await component.getByRole('button', { name: 'Sign in', exact: true }).click();
  await component.getByRole('button', { name: 'Start sign-in', exact: true }).click();
  expect(started).toEqual({ profile: 'central-profile', instance: 'codex-work' });
  await expect(component.getByRole('link', { name: 'Open sign-in page' })).toHaveAttribute('href', repair.url);
  await expect(component.getByLabel('One-time code')).toHaveText('K7QD-M2XPA');
  for (const width of [960, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await expect(component.getByRole('link', { name: 'Open sign-in page' })).toBeVisible();
    await page.screenshot({ path: test.info().outputPath(`named-account-sign-in-${width}.png`) });
  }
  await page.clock.runFor(2_000);
  await expect(component.getByRole('status')).toContainText('Logged in again');
  await expect(component.getByText('Authenticated · isolated state', { exact: true })).toBeVisible();
});

test('closing a named account sign-in cancels its provider process', async ({ mount, page }) => {
  const account = { backend_instance: 'claude-work', runner_kind: 'claude', logical_backend: 'claude', enabled: true,
    isolated_state_configured: true, executable_configured: true, account_label: 'Work', supported_models: [], auth_source_label: null, quota_pool: null } satisfies BackendInstanceSummary;
  await page.route('**/api/backend-instances**', route => route.fulfill({ json: route.request().method() === 'GET'
    ? { profile: 'alpha', backend_instances: [account] }
    : { key: 'secret-key', repair: { id: 'cancel-me', node_id: 'central', backend: 'claude', provider: null, status: 'waiting', expires_at: '' } } }));
  let cancelled = 0;
  await page.route('**/api/auth-health/repairs/cancel-me', route => {
    if (route.request().method() === 'DELETE') {
      expect(route.request().headers()['x-login-repair-key']).toBe('secret-key');
      cancelled++;
    }
    return route.fulfill({ json: { cancelled: true } });
  });
  const component = await mount(<BackendInstancesCard profileName="alpha" effective={{ backend_instances: [account] }} />);
  await component.getByRole('button', { name: 'Sign in', exact: true }).click();
  await component.getByRole('button', { name: 'Start sign-in', exact: true }).click();
  await expect(component.getByRole('status')).toContainText('Waiting for claude');
  await component.getByRole('button', { name: 'Close sign-in' }).click();
  await expect.poll(() => cancelled).toBe(1);
});
