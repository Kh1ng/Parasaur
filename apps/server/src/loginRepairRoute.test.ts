import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { spawn, type SpawnOptions } from 'node:child_process';
import type { LoginRepairView } from '@git-agent-harness/contracts';
import { AuthHealthProber, type AuthHealthMonitor } from './authHealth.js';
import { resetCachedCoordinatorIdentity } from './coordinatorIdentity.js';
import { DEVICE_COOKIE, DeviceAccess } from './deviceAccess.js';
import { LoginRepairBroker, LoginRepairs } from './loginRepair.js';
import type { BackendInstanceRuntime } from './gahCli.js';
import type { RegistryService } from './registryService.js';
import { createServer } from './server.js';

test('only the owner may start a configured named account login, with no client-selected paths', async () => {
  const root = mkdtempSync(join(tmpdir(), 'gah-instance-login-route-'));
  const saved = { ...process.env };
  Object.assign(process.env, { COORDINATOR_TOKEN: 'instance-owner-token', PARASAUR_ALLOW_INSECURE_HTTP: '1',
    PARASAUR_COORDINATOR_IDENTITY_PATH: join(root, 'identity.json'), PARASAUR_MUTATION_STORE_PATH: join(root, 'mutations') });
  resetCachedCoordinatorIdentity();
  const access = new DeviceAccess(join(root, 'devices.json'));
  const calls: string[][] = [];
  const local = new LoginRepairs({ gahBinary: () => '/configured/gah',
    instanceAuth: async (_profile, instance) => ({ backend_instance: instance, auth_ready: true }),
    spawn: ((command: string, args: string[], options: SpawnOptions) => {
      assert.equal(command, '/configured/gah');
      assert.deepEqual(args, ['config', 'authenticate-backend-instance', '--profile', 'alpha', '--instance', 'work']);
      return spawn(process.execPath, ['-e', "console.log('https://auth.openai.com/codex/device K7QD-M2XPA'); setTimeout(() => {}, 500)"], options);
    }) as typeof spawn,
    instanceRuntime: async (profile, instance) => {
    calls.push([profile, instance]);
    return { backend_instance: instance, runner_kind: instance === 'work' ? 'codex' : 'vibe', logical_backend: 'codex', executable: '/tools/provider',
      state_root: '/isolated/vibe', account_label: 'Vibe' } satisfies BackendInstanceRuntime;
  } });
  const broker = new LoginRepairBroker({ localNodeId: 'central', local, registry: {} as RegistryService });
  const server = http.createServer(createServer({ deviceAccess: access, loginRepairBroker: broker }));
  await new Promise<void>(done => server.listen(0, '127.0.0.1', done));
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  const offer = access.create({ id: 'central', name: 'Central', origin });
  const phone = access.redeem(offer.code, 'central', origin, 'Phone');
  let sequence = 0;
  const start = (body: object, owner = true, idempotent = true) => fetch(`${origin}/api/backend-instances/authenticate`, {
    method: 'POST', headers: { Origin: origin, 'X-Forwarded-For': '203.0.113.9', 'Content-Type': 'application/json',
      ...(owner ? { Authorization: 'Bearer instance-owner-token' } : { Cookie: `${DEVICE_COOKIE}=${phone.token}` }),
      ...(idempotent ? { 'Idempotency-Key': `instance-login-test-${++sequence}` } : {}) }, body: JSON.stringify(body)
  });
  try {
    assert.equal((await start({ profile: 'alpha', instance: 'work' }, false)).status, 403);
    assert.equal((await start({ profile: 'alpha', instance: 'work' }, true, false)).status, 400);
    for (const input of [{ profile: '', instance: 'work' }, { profile: 'alpha', instance: 'work', executable: '/evil' },
      { profile: 'alpha', instance: 'work', state_root: '/evil' }, { profile: 'alpha', instance: 'work', node_id: 'other-node' }]) {
      assert.equal((await start(input)).status, 400);
    }
    assert.deepEqual(calls, []);
    const unsupported = await start({ profile: 'alpha', instance: 'vibe' });
    assert.equal(unsupported.status, 409);
    assert.match((await unsupported.json() as { message: string }).message, /supported isolated browser login/);
    assert.deepEqual(calls, [['alpha', 'vibe']]);
    const response = await start({ profile: 'alpha', instance: 'work' });
    assert.equal(response.status, 201);
    const created = await response.json() as { key: string; repair: LoginRepairView };
    assert.equal(created.key.length, 64);
    assert.equal(created.repair.backend, 'codex');
    const headers = { Authorization: 'Bearer instance-owner-token', 'X-Login-Repair-Key': created.key };
    assert.equal((await fetch(`${origin}/api/auth-health/repairs/${created.repair.id}`, { headers })).status, 200);
    assert.equal((await fetch(`${origin}/api/auth-health/repairs/${created.repair.id}`, { method: 'DELETE', headers })).status, 200);
  } finally {
    await new Promise<void>(done => server.close(() => done()));
    process.env = saved;
    resetCachedCoordinatorIdentity();
    rmSync(root, { recursive: true, force: true });
  }
});

test('a paired phone may repair a login it can see, and only it can read that repair (#1272)', async () => {
  const root = mkdtempSync(join(tmpdir(), 'gah-login-repair-route-'));
  const saved = { ...process.env };
  Object.assign(process.env, {
    COORDINATOR_TOKEN: 'repair-route-token', PARASAUR_ALLOW_INSECURE_HTTP: '1',
    PARASAUR_COORDINATOR_IDENTITY_PATH: join(root, 'identity.json'), PARASAUR_MUTATION_STORE_PATH: join(root, 'mutations')
  });
  resetCachedCoordinatorIdentity();
  const access = new DeviceAccess(join(root, 'devices.json'));
  const monitor = { rows: () => [
    { node_id: 'central', node_name: 'Central', backend: 'hermes', provider: null, state: 'missing', source: 'probe' },
    { node_id: 'central', node_name: 'Central', backend: 'claude', backend_instance: 'work', provider: null, state: 'missing', source: 'probe' }
  ] } as unknown as AuthHealthMonitor;
  const broker = new LoginRepairBroker({ localNodeId: 'central', local: new LoginRepairs({ env: { HOME: root, PATH: join(root, 'empty-bin') } }), registry: {} as RegistryService });
  const server = http.createServer(createServer({ deviceAccess: access, authHealthMonitor: monitor, loginRepairBroker: broker }));
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  const device = (name: string) => {
    const offer = access.create({ id: 'central', name: 'Central', origin });
    return access.redeem(offer.code, 'central', origin, name).token;
  };
  const phone = device('Phone');
  const tablet = device('Tablet');
  const as = (token: string, extra: Record<string, string> = {}) => ({ Origin: origin, Cookie: `${DEVICE_COOKIE}=${token}`, 'X-Forwarded-For': '203.0.113.9', ...extra });
  const start = (token: string, login: object, key: string) => fetch(`${origin}/api/auth-health/repairs`, {
    method: 'POST', headers: as(token, { 'Content-Type': 'application/json', 'Idempotency-Key': key }), body: JSON.stringify(login)
  });
  try {
    assert.equal((await start(phone, { node_id: 'central', backend: 'codex', provider: null }, 'repair-start-unknown-0001')).status, 404, 'only logins GAH has seen');
    const named = await start(phone, { node_id: 'central', backend: 'claude', provider: null, backend_instance: 'work' }, 'repair-start-named-0001');
    assert.equal(named.status, 409, 'named logins must never start ambient browser auth');
    const unavailable = await named.json() as { message: string };
    assert.match(unavailable.message, /instance/);
    assert.equal((await start(phone, { node_id: 'central', backend: 'claude', provider: null }, 'repair-start-no-instance-0001')).status, 404, 'a named probe does not authorize ambient repair');
    const started = await start(phone, { node_id: 'central', backend: 'hermes', provider: null }, 'repair-start-hermes-00001');
    assert.equal(started.status, 201, 'a paired device is allowed to start a repair');
    const { key, repair } = await started.json() as { key: string; repair: LoginRepairView };
    assert.equal(repair.status, 'manual');
    const read = (token: string, secret: string) => fetch(`${origin}/api/auth-health/repairs/${repair.id}`, { headers: as(token, { 'X-Login-Repair-Key': secret }) });
    assert.equal((await read(phone, key)).status, 200);
    assert.equal((await read(tablet, key)).status, 404, 'another device cannot read it, even with the key');
    assert.equal((await read(phone, 'a'.repeat(64))).status, 404);

    const prober = new AuthHealthProber(async () => ({ checked_at: 'now', probes: [
      { backend: 'claude', backend_instance: 'work', provider: null, state: 'missing', source: 'probe' }
    ] }));
    await prober.refresh();
    const worker = http.createServer(createServer({ node: { role: 'worker', central_url: origin },
      authHealthProber: prober, loginRepairs: new LoginRepairs({ env: { HOME: root, PATH: join(root, 'empty-bin') } }) }));
    await new Promise<void>((done) => worker.listen(0, '127.0.0.1', done));
    try {
      const endpoint = `http://127.0.0.1:${(worker.address() as AddressInfo).port}/api/login-repair`;
      const repair = (login: object) => fetch(endpoint, { method: 'POST',
        headers: { Authorization: 'Bearer repair-route-token', 'Content-Type': 'application/json' },
        body: JSON.stringify({ action: 'start', backend: 'claude', provider: null, ...login }) });
      assert.equal((await repair({ backend_instance: 'work' })).status, 409);
      assert.equal((await repair({})).status, 404, 'workers also require an ambient probe for ambient repair');
    } finally {
      await new Promise<void>((done) => worker.close(() => done()));
    }
  } finally {
    await new Promise<void>((done) => server.close(() => done()));
    process.env = saved;
    resetCachedCoordinatorIdentity();
    rmSync(root, { recursive: true, force: true });
  }
});
