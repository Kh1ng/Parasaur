import assert from 'node:assert/strict';
import { readFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { after, before, test } from 'node:test';
import { chromium } from '@playwright/test';

const script = readFileSync(new URL('../apps/desktop/dashboard_recovery.js', import.meta.url), 'utf8');
let browser;
before(async () => { browser = await chromium.launch(); });
after(async () => { await browser?.close(); });

async function dashboard(t, content = '<div id="root"></div>', viewport = { width: 1000, height: 700 }) {
  const page = await browser.newPage({ viewport });
  t.after(() => page.close());
  await page.clock.install({ time: new Date('2026-11-03T00:00:00Z') });
  await page.clock.pauseAt(new Date('2026-11-03T00:01:00Z'));
  await page.route('https://central.test/**', route => route.fulfill({ contentType: 'text/html', body: content }));
  await page.goto('https://central.test/?private=do-not-display#private-fragment');
  await page.addScriptTag({ content: script });
  return page;
}

for (const viewport of [{ width: 1000, height: 700 }, { width: 390, height: 844 }]) {
  test(`blank dashboard offers accessible recovery at ${viewport.width}px`, async t => {
    const page = await dashboard(t, undefined, viewport);
    await page.clock.runFor(9999);
    assert.equal(await page.getByRole('heading').count(), 0);
    await page.clock.runFor(1);
    const heading = page.getByRole('heading', { name: 'The dashboard hasn’t started' });
    await heading.waitFor();
    assert.equal(await heading.evaluate(element => element === document.activeElement), true);
    assert.equal(await page.getByLabel('Central address').textContent(), 'https://central.test');
    assert.equal(await page.getByRole('link', { name: 'Open This computer' }).getAttribute('href'), 'gah://connection-recovery');
    assert.equal(await page.locator('body').innerText().then(text => text.includes('do-not-display') || text.includes('private-fragment')), false);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth), false);
    if (process.env.PARASAUR_RECOVERY_SCREENSHOTS) {
      mkdirSync(process.env.PARASAUR_RECOVERY_SCREENSHOTS, { recursive: true });
      await page.screenshot({ path: join(process.env.PARASAUR_RECOVERY_SCREENSHOTS, `${viewport.width}.png`) });
    }
    // Retry navigates back to the same dashboard; it does not change the saved URL.
    let retried = false;
    page.on('request', request => { if (request.isNavigationRequest()) retried = true; });
    await page.getByRole('button', { name: 'Retry dashboard' }).click();
    await page.waitForLoadState();
    assert.equal(retried, true);
  });
}

test('a dashboard that starts during the grace period is left alone', async t => {
  const page = await dashboard(t);
  await page.clock.runFor(9000);
  await page.evaluate(() => { document.getElementById('root').textContent = 'Connected dashboard'; });
  await page.clock.runFor(2000);
  assert.equal(await page.getByRole('heading').count(), 0);
  assert.equal(await page.locator('#root').textContent(), 'Connected dashboard');
});

test('existing app error states and non-dashboard documents are preserved', async t => {
  for (const content of ['<div id="root"><p>Reconnect to central</p></div>', '<main>Sign in to your gateway</main>']) {
    const page = await dashboard(t, content);
    await page.clock.runFor(10000);
    assert.equal(await page.getByRole('heading').count(), 0);
    assert.equal(await page.locator('.gah-dashboard-recovery').count(), 0);
  }
});
