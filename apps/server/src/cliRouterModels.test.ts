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

test('refresh discards an in-flight inventory when the router connection changes', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'router-models-'));
  const configPath = join(dir, 'router-opencode.json');
  let settings = { url: 'http://127.0.0.1:8317', apiKey: 'old-key', managementKey: '' };
  const original = JSON.stringify({ provider: { 'gah-router': { models: { old: { name: 'old' } } } } });
  writeFileSync(configPath, original);
  let respond!: (response: Response) => void;
  const refresher = createCliRouterModelRefresher({ configPath, readSettingsFn: () => settings,
    fetchFn: () => new Promise<Response>(resolve => { respond = resolve; }),
  });
  try {
    const pending = refresher.refresh();
    settings = { ...settings, apiKey: 'new-key' };
    respond(new Response(JSON.stringify({ data: [{ id: 'stale' }] })));
    await pending;
    assert.equal(readFileSync(configPath, 'utf8'), original);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('refresh leaves missing, corrupt, and unrelated provider configurations alone', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'router-models-'));
  const configPath = join(dir, 'router-opencode.json');
  let calls = 0;
  const refresher = createCliRouterModelRefresher({ configPath,
    readSettingsFn: () => ({ url: 'http://127.0.0.1:8317', apiKey: 'secret', managementKey: '' }),
    fetchFn: async () => { calls++; return new Response('{"data":[]}'); },
  });
  try {
    await refresher.refresh();
    for (const original of ['{', JSON.stringify({ provider: { other: { models: {} } } })]) {
      writeFileSync(configPath, original);
      await refresher.refresh();
      assert.equal(readFileSync(configPath, 'utf8'), original);
    }
    assert.equal(calls, 0);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
