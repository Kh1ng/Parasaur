import { expect, test } from '@playwright/experimental-ct-react';
import { NodeFactoryDetails } from '../../src/components/NodeFactoryDetails.js';

const snapshot = (profile = 'project-a') => ({
  node_id: 'worker-one', profile, checked_at: '2026-11-05T12:00:00Z',
  loop: { state: 'available', value: { running: false } },
  decision: { state: 'available', value: { timestamp: '2026-11-05T11:59:00Z', kind: 'human_required', work_id: '#42', reason: 'Review receipt requires an operator decision.', reason_code: 'stuck_loop_gate' } },
  status: { state: 'available', value: { generated_at: '2026-11-05T11:59:00Z', profile_blockers: [], profile_blocker_count: 0,
    work_blockers: [{ work_id: '#42', message: 'The review gate repeated without progress.', reason_code: 'stuck_loop_gate', next_steps: ['Inspect the current review receipt on this worker.'] }], work_blocker_count: 1 } },
});

test('worker profile check explains the controller, decision and work gate without mutations', async ({ mount, page }, testInfo) => {
  const requests: string[] = [];
  await page.route('**/api/registry/nodes/worker-one/factory**', route => {
    requests.push(route.request().method());
    expect(new URL(route.request().url()).searchParams.get('profile')).toBe('project-a');
    return route.fulfill({ json: snapshot() });
  });
  const component = await mount(<NodeFactoryDetails nodeId="worker-one" profiles={['project-a', 'project-b']} />);
  await expect(component.getByText('Controller reported stopped.')).toHaveCount(0);
  await component.getByRole('button', { name: 'Read factory state', exact: true }).click();
  await expect(component).toContainText('Controller reported stopped.');
  await expect(component).toContainText('Review receipt requires an operator decision.');
  await expect(component).toContainText('The review gate repeated without progress.');
  await expect(component).toContainText('Inspect the current review receipt on this worker.');
  await expect(component.getByRole('button', { name: /retry|clear|release|assign/i })).toHaveCount(0);
  expect(requests).toEqual(['GET']);
  await page.screenshot({ path: testInfo.outputPath('worker-factory-desktop.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  expect(await component.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('worker-factory-mobile.png') });
});

test('independent missing observations and failed retries do not imply idle or no blockers', async ({ mount, page }) => {
  let failing = false;
  await page.route('**/api/registry/nodes/worker-one/factory**', route => failing
    ? route.fulfill({ status: 502, json: { message: 'Worker check unavailable' } })
    : route.fulfill({ json: { ...snapshot(), loop: { state: 'unavailable', reason: 'authorization' }, decision: { state: 'available', value: null }, status: { state: 'unavailable', reason: 'timeout' } } }));
  const component = await mount(<NodeFactoryDetails nodeId="worker-one" profiles={['project-a', 'project-b']} />);
  await component.getByRole('button', { name: 'Read factory state', exact: true }).click();
  await expect(component).toContainText('Worker access was denied.');
  await expect(component).toContainText('This does not prove the factory is idle.');
  await expect(component).toContainText('within five seconds');
  await expect(component).not.toContainText('None reported at this check.');
  failing = true;
  await component.getByRole('button', { name: 'Read factory state', exact: true }).click();
  await expect(component.getByRole('alert')).toContainText('current state is unconfirmed');
  await expect(component).toContainText('Last recorded check');
  await component.getByRole('combobox', { name: 'Worker profile' }).selectOption('project-b');
  await expect(component).not.toContainText('Last recorded check');
  await expect(component).not.toContainText('Worker access was denied.');
});

test('a late response from the previous profile cannot restore its gate evidence', async ({ mount, page }) => {
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  await page.route('**/api/registry/nodes/worker-one/factory**', async route => { await gate; await route.fulfill({ json: snapshot() }); });
  const component = await mount(<NodeFactoryDetails nodeId="worker-one" profiles={['project-a', 'project-b']} />);
  await component.getByRole('button', { name: 'Read factory state', exact: true }).click();
  await expect(component.getByRole('button', { name: 'Reading factory state…' })).toBeDisabled();
  await component.getByRole('combobox', { name: 'Worker profile' }).selectOption('project-b');
  const oldResponse = page.waitForResponse(response => response.url().includes('/factory?profile=project-a'));
  release();
  await (await oldResponse).finished();
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));
  await expect(component.getByRole('button', { name: 'Read factory state', exact: true })).toBeEnabled();
  await expect(component).not.toContainText('The review gate repeated without progress.');
  await expect(component.getByRole('alert')).toHaveCount(0);
});
