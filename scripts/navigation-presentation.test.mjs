import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { before, after, test } from 'node:test';
import { chromium } from '@playwright/test';

const script = readFileSync(new URL('../apps/desktop/navigation_presented.js', import.meta.url), 'utf8');
let browser;
before(async () => { browser = await chromium.launch(); });
after(async () => { await browser?.close(); });

test('only visible content in the current top-level document acknowledges presentation', async t => {
  const page = await browser.newPage(); t.after(() => page.close());
  await page.route('https://central.test/**', route => route.fulfill({ contentType: 'text/html', body: '<div id="root"></div>' }));
  await page.goto('https://central.test/?page=chat#turn');
  const expected = { origin: 'https://central.test', path: '/', attempt: '12' };
  const probe = value => page.evaluate(({ script, value }) => new Function('expected', `${script}; return navigationHasContent(expected);`)(value), { script, value });
  assert.equal(await probe(expected), false, 'empty app root is still blank');
  await page.locator('#root').evaluate(root => { root.textContent = 'Dashboard fixture'; });
  assert.equal(await probe(expected), true);
  assert.equal(await probe({ ...expected, origin: 'https://other.test' }), false);
  await page.evaluate(() => history.replaceState(null, '', '/chat?session=current'));
  assert.equal(await probe(expected), true, 'same-document routing keeps existing content');
  await page.setContent('<form><label>Gateway sign in<input></label></form>');
  assert.equal(await probe(expected), true, 'a non-dashboard gateway form must stay available');
  await page.setContent('<body></body>');
  assert.equal(await probe(expected), false);
  await page.setContent('<iframe srcdoc="<div id=root>Embedded content</div>"></iframe>');
  const frame = page.frames().find(frame => frame.parentFrame());
  assert.equal(await frame.evaluate(({ script, value }) => new Function('expected', `${script}; return navigationHasContent(expected);`)(value), { script, value: { ...expected, origin: 'null' } }), false, 'embedded content cannot acknowledge the top-level app');
});

for (const late of [false, true]) {
  test(`presentation observer ${late ? 'expires before late content' : 'acknowledges content during the grace without navigating'}`, async t => {
    const page = await browser.newPage(); t.after(() => page.close());
    await page.clock.install({ time: new Date('2026-11-03T00:00:00Z') });
    await page.clock.pauseAt(new Date('2026-11-03T00:01:00Z'));
    await page.route('https://central.test/**', route => route.fulfill({ contentType: 'text/html', body: '<div id="root"></div>' }));
    await page.goto('https://central.test/');
    await page.evaluate(script => {
      window.calls = [];
      window.__TAURI_INTERNALS__ = { invoke: async (command, args) => { window.calls.push([command, args]); } };
      new Function(`${script}; watchNavigationPresentation({origin: 'https://central.test', path: '/', attempt: '12'});`)();
    }, script);
    await page.clock.runFor(late ? 2100 : 1000);
    await page.locator('#root').evaluate(root => { root.textContent = 'Ready'; });
    await page.evaluate(() => Promise.resolve());
    assert.deepEqual(await page.evaluate(() => window.calls), late ? [] : [['navigation_presented', { attempt: '12' }]]);
    assert.equal(page.url(), 'https://central.test/');
    await page.clock.runFor(2500);
    await page.locator('#root').evaluate(root => { root.textContent = 'Another update'; });
    assert.equal(await page.evaluate(() => window.calls.length), late ? 0 : 1);
  });
}
