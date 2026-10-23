import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createCliRouterModelRefresher } from './cliRouter.js';

test('router models refresh at startup and periodically, preserving config and surviving outages', async t => {
  t.mock.timers.enable({ apis: ['setInterval'] });
  const dir = mkdtempSync(join(tmpdir(), 'router-models-'));
  const configPath = join(dir, 'router-opencode.json');
  const settings = { url: 'http://127.0.0.1:8317', apiKey: 'secret', managementKey: '' };
  const original = { agent: { custom: {} }, provider: {
    other: { models: { untouched: {} } },
    'gah-router': { options: { baseURL: settings.url + '/v1' }, models: { old: { name: 'Custom name' } } },
  } };
  writeFileSync(configPath, JSON.stringify(original));
  let body: unknown = { data: [{ id: 'old' }, { id: 'new' }] };
  let status = 200;
  let calls = 0;
  const refresher = createCliRouterModelRefresher({ configPath, readSettingsFn: () => settings,
    fetchFn: async (url, init) => {
      calls++;
      assert.equal(url, settings.url + '/v1/models');
      assert.equal((init?.headers as Record<string, string>).Authorization, 'Bearer secret');
      return new Response(JSON.stringify(body), { status });
    },
  });
  const read = () => JSON.parse(readFileSync(configPath, 'utf8'));
  const stop = refresher.start();
  try {
    await refresher.refresh();
    assert.equal(calls, 1);
    assert.deepEqual(read().provider['gah-router'].models, { old: { name: 'Custom name' }, new: { name: 'new' } });
    assert.deepEqual(read().agent, original.agent);
    assert.deepEqual(read().provider.other, original.provider.other);
    body = { data: [{ id: 'replacement' }] };
    t.mock.timers.tick(60_000);
    await refresher.refresh();
    assert.deepEqual(read().provider['gah-router'].models, { replacement: { name: 'replacement' } });
    const good = readFileSync(configPath, 'utf8');
    for (const failure of [503, 200]) {
      status = failure;
      body = { error: 'unavailable' };
      await refresher.refresh();
      assert.equal(readFileSync(configPath, 'utf8'), good);
    }
    body = { data: [] };
    await refresher.refresh();
    assert.deepEqual(read().provider['gah-router'].models, {});
    stop();
    const count = calls;
    t.mock.timers.tick(60_000);
    await refresher.refresh();
    assert.equal(calls, count);
    assert.ok(!readFileSync(configPath, 'utf8').includes('secret'));
  } finally { stop(); rmSync(dir, { recursive: true, force: true }); }
});
