import assert from 'node:assert/strict';
import { test } from 'node:test';
import express from 'express';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { RegisteredNode, AgentVersionsSnapshot } from '@git-agent-harness/contracts';
import { AgentCliFleet, agentCliRouter } from './agentCliRoutes.js';
import { AgentCliError, AgentCliVersions } from './agentCliVersions.js';
import { mutationSafety } from './mutationSafety.js';

const node = { node_id: 'local', display_name: 'Local' };
const row = { backend: 'codex', installed: '1.0.0', latest: '1.1.0', outdated: true, source: 'npm:@openai/codex', update_command: 'codex update', error: null };
const options = { checkPolicy: async () => {}, probe: async () => [row], readUpdates: () => [], activeClaims: async () => 0, enabled: () => true, optedIn: () => false, nodeUpdating: () => false };

test('agents routes enforce owner, admin flag, idempotency and valid backend before launching', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-agent-routes-'));
  const old = process.env.GAH_ENABLE_ADMIN_UPDATE;
  let launches = 0;
  const local = new AgentCliVersions(node, { ...options, launch: async () => { launches++; } });
  const app = express(); app.use(express.json());
  app.use((req, res, next) => { res.locals.authPrincipal = { kind: req.get('X-Test-Role') ?? 'owner' }; next(); });
  app.use('/api/agents', agentCliRouter(local, undefined, mutationSafety('local', dir)));
  const server = app.listen(0, '127.0.0.1');
  await new Promise<void>(resolve => server.once('listening', resolve));
  const addr = server.address(); assert.ok(addr && typeof addr !== 'string');
  const base = `http://127.0.0.1:${addr.port}/api/agents`;
  const post = (body: object, headers: Record<string, string> = {}) => fetch(`${base}/update`, { method: 'POST', headers: { 'Content-Type': 'application/json', ...headers }, body: JSON.stringify(body) });
  try {
    delete process.env.GAH_ENABLE_ADMIN_UPDATE;
    assert.equal((await post({ backend: 'codex' })).status, 403);
    process.env.GAH_ENABLE_ADMIN_UPDATE = '1';
    assert.equal((await post({ backend: 'codex' }, { 'X-Test-Role': 'device' })).status, 403);
    assert.equal((await post({ backend: 'codex' })).status, 400);
    assert.equal((await post({ backend: 'codex;sh' }, { 'Idempotency-Key': 'invalid-agent-0001' })).status, 400);
    const inventory = await fetch(`${base}/versions`).then(r => r.json()) as AgentVersionsSnapshot;
    assert.equal(inventory.nodes[0].versions[0].installed, '1.0.0');
    assert.equal(inventory.can_update, true);
    const key = { 'Idempotency-Key': 'valid-agent-update-0001' };
    assert.equal((await post({ backend: 'codex' }, key)).status, 202);
    assert.equal((await post({ backend: 'codex' }, key)).status, 409);
    assert.equal(launches, 1);
  } finally {
    await new Promise<void>(resolve => server.close(() => resolve()));
    if (old === undefined) delete process.env.GAH_ENABLE_ADMIN_UPDATE; else process.env.GAH_ENABLE_ADMIN_UPDATE = old;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('agents fleet includes unavailable nodes and relays only fixed registry destinations', async () => {
  const local = new AgentCliVersions(node, options);
  const worker: RegisteredNode = { node_id: 'worker', display_name: 'Worker', advertised_url: 'http://127.0.0.1:3774',
    version: '1', schema_digest: 'digest', transport_mode: 'authenticated_remote', secret_ref: 'env:AGENT_TEST_TOKEN' };
  const registry = { getNodes: () => [worker], getNode: (id: string) => id === worker.node_id ? worker : undefined };
  const calls: string[] = [];
  const mock: typeof fetch = async (url, init) => {
    calls.push(String(url));
    if (String(url).endsWith('/local')) throw new Error('offline');
    assert.equal(init?.redirect, 'error');
    assert.equal((init?.headers as Record<string, string>)['Authorization'], 'Bearer secret');
    return new Response('{}');
  };
  process.env.AGENT_TEST_TOKEN = 'secret';
  try {
    const fleet = new AgentCliFleet(local, registry, mock);
    const snapshot = await fleet.snapshot();
    assert.equal(snapshot.length, 2); assert.ok(snapshot[1].error);
    await fleet.update('worker', 'codex');
    await assert.rejects(fleet.update('http://attacker', 'codex'));
    await fleet.scheduledUpdate();
    assert.deepEqual(calls, ['http://127.0.0.1:3774/api/agents/local', 'http://127.0.0.1:3774/api/agents/update', 'http://127.0.0.1:3774/api/agents/scheduled-update']);
  } finally { delete process.env.AGENT_TEST_TOKEN; }
});

test('owner and scheduled updates refuse node policy before any installer launch', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-agent-policy-'));
  const old = process.env.GAH_ENABLE_ADMIN_UPDATE;
  let launches = 0;
  const local = new AgentCliVersions(node, { ...options, optedIn: () => true,
    checkPolicy: async () => { throw new AgentCliError(403, 'agent_update_policy_blocked', 'POLICY BLOCKED: update-agent-cli'); },
    launch: async () => { launches++; } });
  const app = express(); app.use(express.json());
  app.use((_req, res, next) => { res.locals.authPrincipal = { kind: 'owner' }; next(); });
  app.use('/api/agents', agentCliRouter(local, undefined, mutationSafety('local', dir)));
  const server = app.listen(0, '127.0.0.1');
  await new Promise<void>(resolve => server.once('listening', resolve));
  const address = server.address(); assert.ok(address && typeof address !== 'string');
  process.env.GAH_ENABLE_ADMIN_UPDATE = '1';
  try {
    for (const route of ['update', 'scheduled-update']) {
      const response = await fetch(`http://127.0.0.1:${address.port}/api/agents/${route}`, {
        method: 'POST', headers: { 'Content-Type': 'application/json', 'Idempotency-Key': `blocked-policy-${route}` },
        body: JSON.stringify({ backend: 'codex', profile: 'readonly' }) });
      assert.equal(response.status, route === 'update' ? 403 : 409);
    }
    await assert.rejects(local.scheduledUpdate(), /POLICY BLOCKED/);
    assert.equal(launches, 0);
  } finally {
    await new Promise<void>(resolve => server.close(() => resolve()));
    if (old === undefined) delete process.env.GAH_ENABLE_ADMIN_UPDATE; else process.env.GAH_ENABLE_ADMIN_UPDATE = old;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('scheduled fleet ticks contain policy refusals and log only state transitions', async (t) => {
  const logs = t.mock.method(console, 'warn', () => {});
  const unhandled: unknown[] = [];
  const onUnhandled = (error: unknown) => { unhandled.push(error); };
  process.on('unhandledRejection', onUnhandled);
  let blocked = true; let launches = 0;
  const local = new AgentCliVersions(node, { ...options, optedIn: () => true,
    checkPolicy: async () => {
      if (blocked) throw new AgentCliError(403, 'agent_update_policy_blocked', 'POLICY BLOCKED: update-agent-cli');
    }, launch: async () => { launches++; } });
  const fleet = new AgentCliFleet(local, { getNodes: () => [], getNode: () => undefined });
  const ticks = async () => {
    // Match bin.ts: the timer discards the promise and relies on the scheduled boundary.
    const timer = setInterval(() => { void fleet.scheduledUpdate(); }, 5 * 60_000);
    try {
      for (let i = 0; i < 3; i++) {
        t.mock.timers.tick(5 * 60_000);
        await new Promise(resolve => setImmediate(resolve));
      }
    } finally { clearInterval(timer); }
  };
  t.mock.timers.enable({ apis: ['setInterval'] });
  try {
    await ticks();
    assert.deepEqual(unhandled, []);
    assert.equal(launches, 0);
    assert.equal(logs.mock.callCount(), 1);
    assert.match(String(logs.mock.calls[0].arguments[0]), /skip.*policy/i);
    blocked = false;
    await ticks();
    assert.ok(launches > 0);
    blocked = true;
    await ticks();
    assert.equal(logs.mock.callCount(), 2, 'refusal after recovery is a new transition');
    assert.deepEqual(unhandled, []);
  } finally { process.off('unhandledRejection', onUnhandled); }
});

test('read-only central still schedules an opted-in worker, which enforces its own policy', async (t) => {
  t.mock.method(console, 'warn', () => {});
  let centralLaunches = 0; let workerLaunches = 0; let workerPolicyChecks = 0;
  const local = new AgentCliVersions(node, { ...options, optedIn: () => true,
    checkPolicy: async () => { throw new AgentCliError(403, 'agent_update_policy_blocked', 'Read-only central'); },
    launch: async () => { centralLaunches++; } });
  const worker: RegisteredNode = { node_id: 'worker', display_name: 'Worker', advertised_url: 'http://127.0.0.1:3774',
    version: '1', schema_digest: 'digest', transport_mode: 'loopback', secret_ref: '' };
  const workerService = new AgentCliVersions(worker, { ...options, optedIn: () => true,
    checkPolicy: async () => { workerPolicyChecks++; }, launch: async () => { workerLaunches++; } });
  const calls: string[] = [];
  const fleet = new AgentCliFleet(local, { getNodes: () => [worker], getNode: () => worker }, async (url, init) => {
    calls.push(String(url));
    assert.equal(init?.method, 'POST');
    await workerService.scheduledUpdate();
    return new Response('{}');
  });
  await fleet.scheduledUpdate();
  assert.deepEqual(calls, ['http://127.0.0.1:3774/api/agents/scheduled-update']);
  assert.equal(centralLaunches, 0);
  assert.equal(workerLaunches, 1);
  assert.equal(workerPolicyChecks, 1);
});

test('scheduled sweep contains registry failures and skips policy checks when not opted in', async (t) => {
  const logs = t.mock.method(console, 'warn', () => {});
  let policyChecks = 0; let registryFailed = true;
  const local = new AgentCliVersions(node, { ...options, checkPolicy: async () => { policyChecks++; } });
  const fleet = new AgentCliFleet(local, {
    getNodes: () => { if (registryFailed) throw new Error('Registry unavailable'); return []; },
    getNode: () => undefined,
  });
  for (let tick = 0; tick < 3; tick++) await assert.doesNotReject(fleet.scheduledUpdate());
  assert.equal(logs.mock.callCount(), 1);
  registryFailed = false;
  await fleet.scheduledUpdate();
  assert.equal(policyChecks, 0);
  registryFailed = true;
  await fleet.scheduledUpdate();
  assert.equal(logs.mock.callCount(), 2);
});
