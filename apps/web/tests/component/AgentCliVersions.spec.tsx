import React from 'react';
import { test, expect } from '@playwright/experimental-ct-react';
import type { AgentVersionsSnapshot } from '@git-agent-harness/contracts';
import { AgentCliVersions } from '../../src/components/AgentCliVersions.js';

function inventory(): AgentVersionsSnapshot {
  return { can_update: true, nodes: [{ node_id: 'worker-1', display_name: 'Build worker', auto_update: false, updates_enabled: true, error: null, updates: [],
    versions: [{ backend: 'codex', installed: '0.110.0', latest: '0.120.0', outdated: true, source: 'npm:@openai/codex', update_command: 'codex update', error: null },
      { backend: 'agy', installed: null, latest: '1.4.7', outdated: false, source: 'release manifest', update_command: 'agy update', error: 'agy is not installed on PATH' }] }] };
}

for (const width of [1280, 390]) {
  test(`CLI versions and update outcome at ${width}px`, async ({ mount, page }) => {
    await page.setViewportSize({ width, height: 900 });
    const data = inventory();
    await page.route('**/api/agents/versions', route => route.fulfill({ json: data }));
    await page.route('**/api/agents/update', async route => {
      expect(route.request().postDataJSON()).toEqual({ node_id: 'worker-1', backend: 'codex' });
      expect(route.request().headers()['idempotency-key']).toBeTruthy();
      data.nodes[0].versions[0] = { ...data.nodes[0].versions[0], installed: '0.120.0', outdated: false };
      data.nodes[0].updates = [{ backend: 'codex', status: 'success', started_at: 'today', finished_at: 'today', before: '0.110.0', after: '0.120.0', error: null }];
      await route.fulfill({ status: 202, json: { accepted: true } });
    });
    const component = await mount(<AgentCliVersions />);
    await expect(component.getByText('0.110.0 / 0.120.0')).toBeVisible();
    await expect(component.getByText('Outdated', { exact: true })).toBeVisible();
    await page.screenshot({ path: `test-results/agent-cli-versions-${width}.png`, fullPage: true });
    await component.getByRole('button', { name: 'Update codex on Build worker' }).click();
    await expect(component.getByText('Last update succeeded · 0.120.0')).toBeVisible();
    await expect(component.getByText('Current', { exact: true })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  });
}

test('read-only owners, offline nodes, and rejected updates remain understandable', async ({ mount, page }) => {
  const data = inventory(); data.can_update = false;
  data.nodes.push({ node_id: 'offline', display_name: 'Offline worker', versions: [], updates: [], auto_update: false, updates_enabled: false, error: 'Node unavailable.' });
  await page.route('**/api/agents/versions', route => route.fulfill({ json: data }));
  const component = await mount(<AgentCliVersions />);
  await expect(component.getByRole('button', { name: /^Update codex/ })).toHaveCount(0);
  await expect(component.getByText('Node unavailable.')).toBeVisible();
  data.can_update = true;
  await component.getByRole('button', { name: 'Refresh versions' }).click();
  await page.route('**/api/agents/update', route => route.fulfill({ status: 409, json: { message: 'Wait for active dispatches to finish.' } }));
  await component.getByRole('button', { name: /^Update codex/ }).click();
  await expect(component.getByRole('alert')).toHaveText('Wait for active dispatches to finish.');
});

test('unknown latest release hides retry after a failed installation', async ({ mount, page }) => {
  const data = inventory();
  data.nodes[0].versions[0].latest = null;
  data.nodes[0].versions[0].error = 'Release manifest unavailable';
  data.nodes[0].updates = [{ backend: 'codex', status: 'failed', started_at: 'today', finished_at: 'today', before: '0.110.0', after: null, error: 'Installer failed' }];
  await page.route('**/api/agents/versions', route => route.fulfill({ json: data }));
  const component = await mount(<AgentCliVersions />);
  await expect(component.getByText('0.110.0 / Unknown')).toBeVisible();
  await expect(component.getByRole('button', { name: /^Update codex/ })).toHaveCount(0);
});
