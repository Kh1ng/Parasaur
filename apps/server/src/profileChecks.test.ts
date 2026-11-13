import assert from 'node:assert/strict';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { createServer } from './server.js';
import { DeviceAccess, DEVICE_COOKIE } from './deviceAccess.js';
import { resetCachedCoordinatorIdentity } from './coordinatorIdentity.js';

test('workspace checks are owner-only, private, and command lists are validated before CLI access', async () => {
  const root = mkdtempSync(join(tmpdir(), 'gah-checks-http-'));
  const saved = { ...process.env };
  const binary = join(root, 'fake-gah');
  writeFileSync(binary, '#!/bin/sh\nexit 91\n'); chmodSync(binary, 0o755);
  Object.assign(process.env, { COORDINATOR_TOKEN: 'checks-owner', PARASAUR_ALLOW_INSECURE_HTTP: '1', PARASAUR_BINARY: binary,
    PARASAUR_COORDINATOR_IDENTITY_PATH: join(root, 'identity.json') });
  resetCachedCoordinatorIdentity();
  const access = new DeviceAccess(join(root, 'devices.json'));
  let reads = 0;
  const server = http.createServer(createServer({ deviceAccess: access, runProfileChecks: async profile => {
    reads++;
    if (profile === 'broken') throw new Error('PRIVATE_CLI_ERROR');
    return { profile, validation_commands: ['echo PRIVATE_COMMAND_CANARY'], auto_fix_commands: [], validation_timeout_seconds: 300 };
  } }));
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  const offer = access.create({ id: 'central', name: 'Central', origin });
  const phone = access.redeem(offer.code, 'central', origin, 'Phone');
  const headers = { Origin: origin, 'X-Forwarded-For': '203.0.113.9', Authorization: 'Bearer checks-owner' };
  try {
    const denied = await fetch(`${origin}/api/profiles/workspace/checks`, { headers: {
      Origin: origin, 'X-Forwarded-For': '203.0.113.9', Cookie: `${DEVICE_COOKIE}=${phone.token}` } });
    assert.equal(denied.status, 403);
    assert.equal(reads, 0);
    const response = await fetch(`${origin}/api/profiles/workspace/checks`, { headers });
    assert.equal(response.status, 200);
    assert.equal(response.headers.get('cache-control'), 'no-store');
    assert.match(JSON.stringify(await response.json()), /PRIVATE_COMMAND_CANARY/);
    assert.equal((await fetch(`${origin}/api/profiles/%00/checks`, { headers })).status, 400);
    assert.equal(reads, 1);
    const failed = await fetch(`${origin}/api/profiles/broken/checks`, { headers });
    assert.equal(failed.status, 502);
    assert.doesNotMatch(await failed.text(), /PRIVATE_CLI_ERROR/);
    for (const body of [{ validation_commands: 'echo unsafe' }, { auto_fix_commands: [false] },
      { validation_commands: [''] }, { validation_commands: ['echo \0bad'] },
      { auto_fix_commands: Array(129).fill('echo test') }, { validation_commands: ['x'.repeat(16_385)] }]) {
      for (const method of ['POST', 'PATCH']) {
        const result = await fetch(`${origin}/api/profiles${method === 'PATCH' ? '/workspace' : ''}`, {
          method, headers: { ...headers, 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
        assert.equal(result.status, 400, `${method}: rejected before the fake CLI could exit 91`);
      }
    }
  } finally {
    await new Promise<void>(resolve => server.close(() => resolve()));
    process.env = saved; resetCachedCoordinatorIdentity(); rmSync(root, { recursive: true, force: true });
  }
});
