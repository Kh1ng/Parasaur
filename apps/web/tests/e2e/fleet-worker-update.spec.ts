import { expect, test } from '@playwright/test';

for (const trigger of ['Update node', 'Update all nodes']) {
  test(`${trigger} follows the selected build through a disconnected restart`, async ({ page }) => {
    const worker = { node_id: 'wsl-build', display_name: 'Windows worker', version: '0.3.0',
      advertised_url: 'http://127.0.0.1:39124', schema_digest: 'fixture', transport_mode: 'loopback',
      update: { status: 'behind', node_version: '0.3.0', coordinator_version: '0.3.0', minimum_worker_version: '0.3.0',
        node_revision: 'a'.repeat(40), coordinator_revision: 'b'.repeat(40) } };
    const state = (status: string) => ({ status, current_version: worker.version, current_revision: (status === 'success' ? 'b' : 'a').repeat(40),
      target_version: null, armed_at: null, started_at: null, finished_at: null, active_dispatches: null, output: '' });
    let starts = 0;
    let polls = 0;
    await page.route('**/api/registry/fleet/snapshot', route => route.fulfill({ json: { nodes: [worker], observations: [], leases: [] } }));
    await page.route('**/api/registry/fleet/update-all', route => {
      starts++;
      return route.fulfill({ json: { results: [{ node_id: worker.node_id, display_name: worker.display_name,
        started: true, status: state('running'), error: null }] } });
    });
    await page.route('**/api/registry/nodes/wsl-build/update', route => {
      if (route.request().method() === 'POST') {
        starts++;
        return route.fulfill({ status: 202, json: state('running') });
      }
      if (!starts) return route.fulfill({ json: state('idle') });
      polls++;
      if (polls === 2) return route.fulfill({ status: 502, json: { message: 'Worker is restarting' } });
      return route.fulfill({ json: state(polls === 1 ? 'inferred_restart' : 'success') });
    });
    await page.goto('/');
    await page.getByRole('navigation', { name: 'Primary' }).getByRole('button', { name: 'Fleet', exact: true }).click();
    await expect(page.getByText('Update available · aaaaaaaa → bbbbbbbb')).toBeVisible();
    await page.getByRole('button', { name: trigger, exact: true }).click();
    await expect(page.getByText('Waiting for the worker to restart')).toBeVisible({ timeout: 8_000 });
    await expect(page.getByRole('button', { name: 'Updating…', exact: true })).toBeDisabled();
    await expect(page.getByText('Reconnecting to the worker…')).toBeVisible({ timeout: 8_000 });
    await expect(page.getByText('Updated to v0.3.0 · bbbbbbbb')).toBeVisible({ timeout: 8_000 });
    await expect(page.getByRole('button', { name: 'Update node', exact: true })).toBeEnabled();
    expect(starts).toBe(1);
  });
}
