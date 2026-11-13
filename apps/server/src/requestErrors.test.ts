import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { test } from 'node:test';
import { withFixtureServer } from './fixtureGahHarness.js';
import { createServer } from './server.js';
import { requestFailure } from './requestFailure.js';

test('JSON parser errors retain client status without logging or returning request contents', async (t) => {
  const logged: unknown[][] = [];
  t.mock.method(console, 'error', (...args: unknown[]) => logged.push(args));
  await withFixtureServer(async base => {
    for (const [body, status] of [
      ['{"token":"test-body-secret",', 400],
      ['"test-body-secret"', 400],
      ['null', 400],
      [JSON.stringify({ token: 'test-body-secret', padding: 'x'.repeat(110_000) }), 413]
    ] as const) {
      const response = await fetch(`${base}/api/git/pr`, {
        method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: 'Bearer test-header-secret' }, body
      });
      assert.equal(response.status, status);
      const text = await response.text();
      assert.ok(!text.includes('test-body-secret'));
      assert.ok(!text.includes('test-header-secret'));
    }
    const unsupported = await fetch(`${base}/api/git/pr`, {
      method: 'POST', headers: { 'Content-Type': 'application/json; charset=ascii' },
      body: '{"token":"test-body-secret"}'
    });
    assert.equal(unsupported.status, 415);
    assert.deepEqual(await unsupported.json(), { error: 'Unsupported Media Type', message: 'Unsupported request encoding' });
  });
  assert.deepEqual(logged, [], 'invalid input is not a server failure and must not log request bodies');
});

test('unexpected failures remain 500 without exposing error contents in responses or logs', async (t) => {
  const logged: unknown[][] = [];
  t.mock.method(console, 'error', (...args: unknown[]) => logged.push(args));
  const saved = process.env.PARASAUR_ENABLE_ADMIN_UPDATE;
  process.env.PARASAUR_ENABLE_ADMIN_UPDATE = '1';
  t.after(() => {
    if (saved === undefined) delete process.env.PARASAUR_ENABLE_ADMIN_UPDATE;
    else process.env.PARASAUR_ENABLE_ADMIN_UPDATE = saved;
  });
  const app = createServer({ getPendingCommits: () => {
    throw Object.assign(new Error('test-internal-secret'), { code: 'EACCES', syscall: 'open', path: '/test-private-path' });
  } });
  const server = http.createServer(app);
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise<void>(resolve => server.close(() => resolve())));
  const { port } = server.address() as AddressInfo;
  const response = await fetch(`http://127.0.0.1:${port}/api/admin/update`);
  assert.equal(response.status, 500);
  assert.deepEqual(await response.json(), { error: 'Internal Server Error', message: 'An unexpected error occurred' });
  assert.deepEqual(logged, [['Server error: an unexpected request failure occurred', {
    surface: 'api', code: 'EACCES', operation: 'open'
  }]]);
});

test('failure summaries retain only known system codes and fixed request categories', () => {
  assert.deepEqual(requestFailure({ code: 'EIO', syscall: 'read', message: 'private-token', path: '/private-file' }, '/assets/private-name.js?token=secret'), {
    surface: 'dashboard_asset', code: 'EIO', operation: 'read'
  });
  for (const error of [null, 'private-error', { code: 'private-token', syscall: 'private-path' }, new Error('private-error')]) {
    assert.deepEqual(requestFailure(error, '/private-conversation'), {
      surface: 'document_or_other', code: 'UNKNOWN', operation: 'unknown'
    });
  }
  assert.equal(requestFailure({}, '/api').surface, 'api');
  assert.equal(requestFailure({}, '/api/private-name').surface, 'api');
  assert.equal(requestFailure({}, '/api-not-a-route').surface, 'document_or_other');
});

test('an asset can pass HEAD and fail during GET; logs identify the read failure without private paths', async (t) => {
  const root = fs.mkdtempSync(join(tmpdir(), 'parasaur-asset-error-'));
  fs.mkdirSync(join(root, 'assets'));
  fs.writeFileSync(join(root, 'assets/private-name.js'), 'window.test = true;');
  const previous = process.env.PARASAUR_WEB_ROOT;
  process.env.PARASAUR_WEB_ROOT = root;
  t.after(() => {
    if (previous === undefined) delete process.env.PARASAUR_WEB_ROOT;
    else process.env.PARASAUR_WEB_ROOT = previous;
    fs.rmSync(root, { recursive: true, force: true });
  });
  const logged: unknown[][] = [];
  t.mock.method(console, 'error', (...args: unknown[]) => logged.push(args));
  const stream = t.mock.method(fs, 'createReadStream', () => new Readable({
    read() {
      this.destroy(Object.assign(new Error('private-read-error'), { code: 'EACCES', syscall: 'open', path: '/private-file' }));
    }
  }));
  const server = http.createServer(createServer());
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise<void>(resolve => server.close(() => resolve())));
  const { port } = server.address() as AddressInfo;
  const url = `http://127.0.0.1:${port}/assets/private-name.js?token=private-token`;
  assert.equal((await fetch(url, { method: 'HEAD' })).status, 200);
  assert.equal(stream.mock.callCount(), 0, 'HEAD does not establish readable file contents');
  const response = await fetch(url);
  assert.equal(response.status, 500);
  assert.deepEqual(await response.json(), { error: 'Internal Server Error', message: 'An unexpected error occurred' });
  assert.deepEqual(logged, [['Server error: an unexpected request failure occurred', {
    surface: 'dashboard_asset', code: 'EACCES', operation: 'open'
  }]]);
});
