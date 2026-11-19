import assert from 'node:assert/strict';
import { createServer as httpServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { Router } from 'express';
import { createServer } from './server.js';
import { DeviceAccess, DEVICE_COOKIE } from './deviceAccess.js';
import { FIXTURE_GAH_BINARY } from './fixtureGahHarness.js';
import { resetCachedCoordinatorIdentity } from './coordinatorIdentity.js';

test('trusted owner extensions share authentication, reject devices and remain reachable on workers', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'gah-owner-routes-'));
  const saved = { PARASAUR_BINARY: process.env.PARASAUR_BINARY, COORDINATOR_TOKEN: process.env.COORDINATOR_TOKEN,
    PARASAUR_COORDINATOR_IDENTITY_PATH: process.env.PARASAUR_COORDINATOR_IDENTITY_PATH };
  Object.assign(process.env, { PARASAUR_BINARY: FIXTURE_GAH_BINARY, COORDINATOR_TOKEN: 'owner-extension-fixture',
    PARASAUR_COORDINATOR_IDENTITY_PATH: join(directory, 'identity.json') });
  resetCachedCoordinatorIdentity();
  try {
    for (const role of ['central', 'worker'] as const) {
      const devices = new DeviceAccess(join(directory, `devices-${role}.json`));
      const identity = { id: 'fixture-node', name: 'Fixture', origin: 'https://fixture.test' };
      const offer = devices.create(identity), paired = devices.redeem(offer.code, identity.id, identity.origin, 'Fixture phone');
      let calls = 0;
      const app = createServer({ node: { role, central_url: role === 'worker' ? 'https://central.test' : null }, deviceAccess: devices,
        ownerRoutes: () => {
          const router = Router(); router.all('/fixture', (_req, res) => { calls++; res.json({ owner: true }); }); return router;
        } });
      const server = httpServer(app); await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
      const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
      const remote = { 'X-Forwarded-For': '198.51.100.9', 'X-Forwarded-Proto': 'https' };
      try {
        assert.equal((await fetch(`${base}/api/owner/fixture`, { headers: remote })).status, 401);
        const device = await fetch(`${base}/api/owner/fixture`, { headers: { ...remote,
          Cookie: `${DEVICE_COOKIE}=${paired.token}`, Origin: base.replace('http:', 'https:') } });
        assert.equal(device.status, role === 'central' ? 403 : 401); assert.equal(calls, 0);
        assert.equal((await fetch(`${base}/api/owner/fixture`, { headers: { ...remote,
          Authorization: 'Bearer owner-extension-fixture' } })).status, 200);
        assert.equal(calls, 1);
        assert.equal((await fetch(`${base}/api/missing-extension`, { headers: { ...remote,
          Authorization: 'Bearer owner-extension-fixture' } })).status, role === 'worker' ? 409 : 404);
      } finally { server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); }
    }
  } finally {
    for (const [key, value] of Object.entries(saved)) { if (value === undefined) delete process.env[key]; else process.env[key] = value; }
    resetCachedCoordinatorIdentity(); rmSync(directory, { recursive: true, force: true });
  }
});
