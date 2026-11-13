import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer as createHttpServer, type Server } from 'node:http';
import express from 'express';
import rateLimit from 'express-rate-limit';
import { chmodSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { validateNodeRole, workerMemoryEnvironment, workerRouteGuard } from './nodeRole.js';
import { workerMemoryRouter } from './workerMemory.js';
import { RegistryService } from './registryService.js';
import { authMiddleware } from './authMiddleware.js';
import { resetCachedCoordinatorIdentity } from './coordinatorIdentity.js';
import { initializeSkillBank, createServer } from './server.js';
import type { DoctorSnapshot, NodeRoleStatus, StatusSnapshot } from '@git-agent-harness/contracts';

async function listen(server: Server): Promise<string> {
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  return `http://127.0.0.1:${address.port}`;
}
const close = (server: Server) => new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
const worker = { role: 'worker', central_url: 'https://central.test' } as const;

test('status preserves fresh CLI claim evidence without replacing the configured node identity', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'gah-claim-status-'));
  const savedBinary = process.env.PARASAUR_BINARY;
  const savedIdentity = process.env.PARASAUR_COORDINATOR_IDENTITY_PATH;
  const fixture = join(directory, 'status.json');
  const binary = join(directory, 'gah');
  writeFileSync(binary, `#!/usr/bin/env node\nprocess.stdout.write(require('node:fs').readFileSync(${JSON.stringify(fixture)},'utf8'));\n`);
  chmodSync(binary, 0o755);
  process.env.PARASAUR_BINARY = binary;
  process.env.PARASAUR_COORDINATOR_IDENTITY_PATH = join(directory, 'identity.json');
  resetCachedCoordinatorIdentity();
  const server = createHttpServer(createServer({ node: worker }));
  const url = await listen(server);
  try {
    for (const [index, origin] of ['https://central.test', 'https://another.test'].entries()) {
      const observation = { state: 'observed', outcome: 'refused', observed_at: '2026-11-03T12:00:00Z', work_id: '#1516' };
      writeFileSync(fixture, JSON.stringify({ generated_at: '2026-11-03T12:00:00Z', active_claims: [], running_workers: [], node: { role: 'central', central_url: origin, central_claim_observation: observation } }));
      const response = await fetch(`${url}/api/status?profile=claim-status-${index}`);
      assert.equal(response.status, 200);
      const status = await response.json() as StatusSnapshot;
      assert.equal(status.node?.role, 'worker', 'the server owns role policy');
      assert.equal(status.node?.central_url, worker.central_url);
      assert.deepEqual(status.node?.central_claim_observation, index === 0 ? observation : undefined, 'a different central cannot supply this node’s claim history');
    }
  } finally {
    await close(server);
    if (savedBinary === undefined) delete process.env.PARASAUR_BINARY; else process.env.PARASAUR_BINARY = savedBinary;
    if (savedIdentity === undefined) delete process.env.PARASAUR_COORDINATOR_IDENTITY_PATH; else process.env.PARASAUR_COORDINATOR_IDENTITY_PATH = savedIdentity;
    resetCachedCoordinatorIdentity();
    rmSync(directory, { recursive: true, force: true });
  }
});

test('worker role reports central identity and permits execution without hosting central stores', async () => {
  assert.throws(() => validateNodeRole({ role: 'worker', central_url: null }), /requires/);
  assert.throws(() => validateNodeRole({ role: 'worker', central_url: 'https://user:secret@central.test' }), /credentials/);
  assert.throws(() => validateNodeRole({ role: 'central', central_url: 'https://user:secret@central.test' }), /credentials/);
  assert.throws(() => initializeSkillBank(worker), /must not initialize/);
  const localOnlyRegistry = new RegistryService(null);
  assert.deepEqual(localOnlyRegistry.getNodesSummary(), []);
  assert.throws(() => workerMemoryEnvironment(worker, undefined), /COORDINATOR_TOKEN/);
  assert.deepEqual(workerMemoryEnvironment(worker, 'node-token'), { TDAI_GATEWAY_URL: 'https://central.test/api/worker-memory', TDAI_GATEWAY_API_KEY: 'node-token' });
  const app = createServer({ node: worker, runDoctor: async () => ({ schema_version: 1, generated_at: '2026-09-07T00:00:00Z', overall_status: 'ok', checks: [] } satisfies DoctorSnapshot) });
  const server = createHttpServer(app);
  const url = await listen(server);
  try {
    const health = await (await fetch(`${url}/health`)).json() as { node: unknown };
    assert.deepEqual(health.node, worker);
    for (const path of ['/api/settings/nodes/command', '/api/skills', '/api/claims/acquire', '/api/registry/nodes', '/api/manager-chat/sessions', '/api/worker-memory/recall', '/api/pm/plans']) {
      const response = await fetch(url + path);
      assert.equal(response.status, 409, path);
      assert.equal((await response.json() as { central_url: string }).central_url, worker.central_url);
    }
    assert.equal((await fetch(`${url}/api/doctor?profile=test`)).status, 200);
    // A worker has no dashboard; `gah diag export` reads its bundle locally.
    const diagnostics = await fetch(`${url}/api/diagnostics/export`);
    assert.equal(diagnostics.status, 200);
    assert.equal(diagnostics.headers.get('content-type'), 'application/gzip');
    // Invalid dispatch reaches execution validation; it never starts a backend.
    assert.equal((await fetch(`${url}/api/dispatch`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' })).status, 400);
    const probe = express();
    probe.use(workerRouteGuard(worker));
    probe.get('/api/status', (_req, res) => res.sendStatus(200));
    probe.all('/api/loop/last-decision', (_req, res) => res.json(null));
    const statusServer = createHttpServer(probe);
    const statusUrl = await listen(statusServer);
    try {
      assert.equal((await fetch(`${statusUrl}/api/status`)).status, 200);
      assert.equal((await fetch(`${statusUrl}/api/loop/last-decision?profile=test`)).status, 200);
      assert.equal((await fetch(`${statusUrl}/api/loop/last-decision`, { method: 'POST' })).status, 409);
    } finally { await close(statusServer); }
  } finally {
    await close(server);
  }
});

test('standalone role boots the local control plane instead of failing closed', async () => {
  const standalone = { role: 'standalone', central_url: null } as const;
  // The narrowest common call path accepts the role and normalizes it to central.
  assert.deepEqual(validateNodeRole(standalone), { role: 'central', central_url: null });
  // A future role this server does not know still fails closed with an upgrade instruction.
  assert.throws(() => validateNodeRole({ role: 'edge', central_url: null } as unknown as NodeRoleStatus), /Cannot determine node role/);
  assert.throws(() => validateNodeRole({ role: 'standalone', central_url: 'https://user:secret@central.test' } as unknown as NodeRoleStatus), /credentials/);
  const app = createServer({ node: standalone, runDoctor: async () => ({ schema_version: 1, generated_at: '2026-10-04T00:00:00Z', overall_status: 'ok', checks: [] } satisfies DoctorSnapshot) });
  const server = createHttpServer(app);
  const url = await listen(server);
  try {
    const health = await (await fetch(`${url}/health`)).json() as { node: unknown };
    assert.deepEqual(health.node, { role: 'central', central_url: null });
    // Central routes answer through their handlers instead of the worker guard's 409.
    for (const path of ['/api/settings/nodes/command', '/api/claims/acquire', '/api/registry/nodes', '/api/manager-chat/sessions', '/api/pm/plans']) {
      assert.notEqual((await fetch(url + path)).status, 409, path);
    }
    const probe = express();
    probe.use(workerRouteGuard(standalone));
    probe.get('/api/status', (_req, res) => res.sendStatus(200));
    const statusServer = createHttpServer(probe);
    const statusUrl = await listen(statusServer);
    try { assert.equal((await fetch(`${statusUrl}/api/status`)).status, 200); } finally { await close(statusServer); }
  } finally {
    await close(server);
  }
});

test('central memory relay preserves gateway contracts without forwarding worker credentials or arbitrary destinations', async () => {
  const previous = { url: process.env.TDAI_GATEWAY_URL, key: process.env.TDAI_GATEWAY_API_KEY, settings: process.env.PARASAUR_GATEWAY_SETTINGS_PATH, token: process.env.COORDINATOR_TOKEN };
  const directory = mkdtempSync(join(tmpdir(), 'gah-relay-'));
  const received: { path?: string; authorization?: string; body?: unknown }[] = [];
  const gateway = express();
  gateway.use(express.json());
  gateway.post('*', (req, res) => {
    received.push({ path: req.path, authorization: req.headers.authorization, body: req.body });
    res.json(req.path === '/recall' ? { code: 0, context: 'shared memory', memory_count: 1 } : { l0_recorded: 1 });
  });
  const gatewayServer = createHttpServer(gateway);
  process.env.TDAI_GATEWAY_URL = await listen(gatewayServer);
  process.env.TDAI_GATEWAY_API_KEY = 'central-gateway-key';
  process.env.COORDINATOR_TOKEN = 'worker-token';
  process.env.PARASAUR_GATEWAY_SETTINGS_PATH = join(directory, 'settings.json');
  const app = express();
  app.use(express.json());
  app.use('/api/worker-memory', rateLimit({ windowMs: 60_000, limit: 100 }), authMiddleware, workerMemoryRouter());
  const server = createHttpServer(app);
  const url = await listen(server);
  try {
    for (const [operation, body] of [
      ['recall', { session_key: 'gah:worker:owner/repo:#1', query: 'previous work' }],
      ['capture', { session_key: 'gah:worker:owner/repo:#1', user_content: 'task', assistant_content: 'result' }],
      ['session/end', { session_key: 'gah:worker:owner/repo:#1' }],
    ] as const) {
      const response = await fetch(`${url}/api/worker-memory/${operation}`, { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: 'Bearer worker-token' }, body: JSON.stringify({ ...body, profile: 'worker-project', url: 'https://attacker.test', token: 'worker-token' }) });
      assert.equal(response.status, 200);
      assert.deepEqual(received.at(-1), { path: `/${operation}`, authorization: 'Bearer central-gateway-key', body });
      assert.ok(!JSON.stringify(await response.json()).includes('central-gateway-key'));
    }
    const bad = await fetch(`${url}/api/worker-memory/recall`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ session_key: 'other', query: 'x' }) });
    assert.equal(bad.status, 400);
    assert.equal((await fetch(`${url}/api/worker-memory/arbitrary`, { method: 'POST' })).status, 404);
    assert.equal(received.length, 3);
    for (const settings of [{ enabled: false }, { disabledProfiles: ['worker-project'] }]) {
      writeFileSync(process.env.PARASAUR_GATEWAY_SETTINGS_PATH!, JSON.stringify(settings));
      const skipped = await fetch(`${url}/api/worker-memory/capture`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ profile: 'worker-project', session_key: 'gah:worker:owner/repo:#1', user_content: 'task', assistant_content: 'result' }) });
      assert.equal(skipped.status, 200);
      assert.equal((await skipped.json() as { l0_recorded: number }).l0_recorded, 0);
      assert.equal(received.length, 3, 'Disabled memory must not contact the gateway');
    }
  } finally {
    await close(server);
    await close(gatewayServer);
    rmSync(directory, { recursive: true });
    for (const [key, value] of Object.entries({ TDAI_GATEWAY_URL: previous.url, TDAI_GATEWAY_API_KEY: previous.key, PARASAUR_GATEWAY_SETTINGS_PATH: previous.settings, COORDINATOR_TOKEN: previous.token })) {
      if (value === undefined) delete process.env[key]; else process.env[key] = value;
    }
  }
});
