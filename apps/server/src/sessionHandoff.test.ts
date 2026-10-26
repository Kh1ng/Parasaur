import assert from 'node:assert/strict';
import { test } from 'node:test';
import http from 'node:http';
import express from 'express';
import { sessionHandoffRouter } from './sessionHandoff.js';

test('handoff requires owner approval, the initiating verifier, TLS/origin, and a single claim within sixty seconds', async () => {
  const previous = process.env.COORDINATOR_TOKEN;
  process.env.COORDINATOR_TOKEN = 'handoff-owner-fixture';
  let now = Date.now();
  const app = express();
  app.set('trust proxy', 'loopback');
  app.use(express.json());
  app.use('/handoff', sessionHandoffRouter(() => now));
  const server = http.createServer(app);
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address(); assert.ok(address && typeof address !== 'string');
  const base = `http://127.0.0.1:${address.port}`;
  const remote = { 'X-Forwarded-For': '198.51.100.2', 'X-Forwarded-Proto': 'https', Origin: base.replace('http:', 'https:') };
  const post = (path: string, body: unknown, headers = remote) => fetch(base + '/handoff' + path,
    { method: 'POST', headers: { ...headers, 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  const owner = { ...remote, Authorization: 'Bearer handoff-owner-fixture' };
  try {
    assert.equal((await post('', {}, { ...remote, Origin: 'https://evil.test' })).status, 403);
    assert.equal((await post('', {}, { ...remote, 'X-Forwarded-Proto': 'http' })).status, 403);
    const created = await post('', {});
    assert.equal(created.headers.get('cache-control'), 'no-store');
    const first = await created.json() as { code: string; verifier: string; expires_in: number };
    assert.equal(first.expires_in, 60);
    assert.deepEqual(await (await post('/claim', first)).json(), { pending: true });
    assert.equal((await post('/approve', first)).status, 401);
    assert.equal((await post('/approve', { code: first.code }, owner)).status, 200);
    assert.equal((await post('/approve', { code: first.code }, owner)).status, 410);
    const second = await (await post('', {})).json() as typeof first;
    // A separate tab with the public code but its own verifier cannot steal it.
    assert.equal((await post('/claim', { code: first.code, verifier: second.verifier })).status, 410);
    assert.equal((await post('/claim', { code: first.code })).status, 410);
    const claimed = await post('/claim', first);
    assert.equal(claimed.status, 200);
    assert.deepEqual(await claimed.json(), { token: 'handoff-owner-fixture' });
    assert.equal((await post('/claim', first)).status, 410);
    assert.equal((await post('/approve', { code: first.code }, owner)).status, 410);
    assert.equal((await post('/approve', { code: second.code }, owner)).status, 200);
    const rotated = await (await post('', {})).json() as typeof first;
    assert.equal((await post('/approve', { code: rotated.code }, owner)).status, 200);
    process.env.COORDINATOR_TOKEN = 'rotated-owner-fixture';
    assert.equal((await post('/claim', rotated)).status, 410);
    process.env.COORDINATOR_TOKEN = 'handoff-owner-fixture';
    now += 60_000;
    assert.equal((await post('/claim', second)).status, 410);
    assert.equal((await post('/approve', { code: second.code }, owner)).status, 410);
  } finally {
    await new Promise<void>(resolve => server.close(() => resolve()));
    if (previous === undefined) delete process.env.COORDINATOR_TOKEN; else process.env.COORDINATOR_TOKEN = previous;
  }
});
