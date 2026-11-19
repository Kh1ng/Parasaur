import { expect, test } from '@playwright/experimental-ct-react';
import { GahUpdateBanner } from '../../src/components/GahUpdateBanner.js';

const release = { channel: 'edge', current_version: '0.3.0', latest_version: '0.4.0', update_available: true, release_url: null, published_at: null, notes: '' };
const state = (status: 'idle' | 'running' | 'failed' | 'success' | 'inferred_restart') => ({ status, startedAt: null, finishedAt: null, exitCode: null, pid: null, output: '' });

test('an equal-version edge update displays both builds and submits the offered revision', async ({ mount, page }) => {
  const selected = 'b'.repeat(40);
  let request;
  await page.route('**/api/admin/release/status', route => route.fulfill({json:{
    ...release, latest_version:'0.3.0', current_commit:'a'.repeat(40), latest_commit:selected
  }}));
  await page.route('**/api/admin/update/status', route => route.fulfill({json:state('idle')}));
  await page.route('**/api/admin/update', route => {
    request = route.request().postDataJSON();
    return route.fulfill({status:202,json:state('running')});
  });
  const banner = await mount(<GahUpdateBanner />);
  await expect(banner).toContainText('v0.3.0 (aaaaaaaa) → v0.3.0 (bbbbbbbb)');
  await page.setViewportSize({width:390,height:350});
  expect(await banner.evaluate(el => el.scrollWidth <= el.clientWidth)).toBe(true);
  await banner.getByRole('button',{name:'Update server',exact:true}).click();
  await expect.poll(() => request).toEqual({mode:'release',expected_commit:selected});
});

test('when the feed moves the banner shows the replacement and waits for another click', async ({ mount, page }) => {
  let revision = 'b'.repeat(40);
  const attempts: string[] = [];
  await page.route('**/api/admin/release/status', route => route.fulfill({
    json: { ...release, current_commit: 'a'.repeat(40), latest_commit: revision }
  }));
  await page.route('**/api/admin/update/status', route => route.fulfill({ json: state('idle') }));
  await page.route('**/api/admin/update', route => {
    attempts.push(route.request().postDataJSON().expected_commit);
    revision = 'c'.repeat(40);
    return route.fulfill({ status: 412, json: { message: 'The published build changed.' } });
  });
  const banner = await mount(<GahUpdateBanner />);
  await expect(banner).toContainText('bbbbbbbb');
  await banner.getByRole('button', { name: 'Update server', exact: true }).click();
  await expect(banner).toContainText('cccccccc');
  await expect(banner).toContainText('The published build changed.');
  expect(attempts).toEqual(['b'.repeat(40)]);
});

test('reopened dashboard resumes a running server update without another install', async ({ mount, page }, testInfo) => {
  let reads = 0;
  let installs = 0;
  await page.route('**/api/admin/release/status', route => route.fulfill({ json: { ...release, update_available: false } }));
  await page.route('**/api/admin/update', route => { installs++; return route.fulfill({ status: 202, json: state('running') }); });
  await page.route('**/api/admin/update/status', route => route.fulfill({ json: state(++reads === 1 ? 'running' : 'failed') }));
  const component = await mount(<GahUpdateBanner />);
  await expect(component).toContainText('Server update failed', { timeout: 7000 });
  await expect(component).toContainText('Details in Settings');
  expect(reads).toBe(2);
  expect(installs).toBe(0);
  for (const width of [1100, 390]) {
    await page.setViewportSize({ width, height: 300 });
    expect(await component.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
    await component.screenshot({ path: testInfo.outputPath(`server-update-${width}.png`) });
  }
});

test('navigating away cancels scheduled update status reads', async ({ mount, page }) => {
  await page.clock.install();
  let reads = 0;
  await page.route('**/api/admin/release/status', route => route.fulfill({ json: release }));
  await page.route('**/api/admin/update/status', route => { reads++; return route.fulfill({ json: state('idle') }); });
  await page.route('**/api/admin/update', route => route.fulfill({ status: 202, json: state('running') }));
  const component = await mount(<GahUpdateBanner />);
  await component.getByRole('button', { name: /Restart to update|Update server/ }).click();
  await expect(component).toContainText('Installing');
  await component.unmount();
  const before = reads;
  await page.clock.runFor(6000);
  // Let browser fetches triggered by the virtual timers reach route handlers.
  await page.waitForTimeout(200);
  expect(reads).toBe(before);
});

test('an interrupted status read stays unconfirmed until a terminal result returns', async ({ mount, page }) => {
  await page.clock.install();
  let reads = 0;
  await page.route('**/api/admin/release/status', route => route.fulfill({ json: release }));
  await page.route('**/api/admin/update/status', route => {
    reads++;
    return reads === 2 ? route.abort('failed') : route.fulfill({ json: state(reads === 1 ? 'running' : 'failed') });
  });
  const component = await mount(<GahUpdateBanner />);
  await expect(component).toContainText('Installing release');
  await page.clock.runFor(2000);
  await expect(component).toContainText('Server update status unavailable. Reconnecting…');
  await expect(component.getByRole('button', { name: /Update server|Retry server update/ })).toHaveCount(0);
  await page.clock.runFor(2000);
  await expect(component).toContainText('Server update failed');
  await expect(component).not.toContainText('Reconnecting');
  const before = reads;
  await page.clock.runFor(6000);
  await page.waitForTimeout(200);
  expect(reads).toBe(before);
});

test('a late startup read cannot replace a newly started update with idle', async ({ mount, page }) => {
  let releaseRead!: () => void;
  const held = new Promise<void>(resolve => { releaseRead = resolve; });
  await page.route('**/api/admin/release/status', route => route.fulfill({ json: release }));
  await page.route('**/api/admin/update/status', async route => { await held; await route.fulfill({ json: state('idle') }); });
  await page.route('**/api/admin/update', route => route.fulfill({ status: 202, json: state('running') }));
  const component = await mount(<GahUpdateBanner />);
  await component.getByRole('button', { name: 'Update server', exact: true }).click();
  await expect(component).toContainText('Installing release');
  const startupResponse = page.waitForResponse('**/api/admin/update/status');
  releaseRead();
  await (await startupResponse).finished();
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => resolve())));
  await expect(component).toContainText('Installing release');
  await expect(component.getByRole('button', { name: 'Update server', exact: true })).toHaveCount(0);
});

for (const terminal of ['success', 'inferred_restart'] as const) {
  test(`a resumed update reloads the dashboard after ${terminal}`, async ({ mount, page }) => {
    let reads = 0;
    await page.route('**/api/admin/release/status', route => route.fulfill({ json: release }));
    await page.route('**/api/admin/update/status', route => route.fulfill({ json: state(++reads === 1 ? 'running' : terminal) }));
    const component = await mount(<GahUpdateBanner />);
    await expect(component).toContainText('Installing release');
    await page.waitForEvent('framenavigated', { predicate: frame => frame === page.mainFrame() });
    expect(reads).toBe(2);
  });
}
