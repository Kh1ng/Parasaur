import assert from 'node:assert/strict';
import { test, type TestContext } from 'node:test';
import http from 'node:http';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { NodeFactorySnapshot, RegisteredNode } from '@git-agent-harness/contracts';
import { readNodeFactory, NodeFactoryError } from './nodeFactory.js';
import { RegistryService } from './registryService.js';
import { COORDINATOR_SCHEMA_DIGEST } from './coordinatorIdentity.js';
import { createServer } from './server.js';

const at = '2026-11-05T12:00:00Z';
const status = { profile: { profile: 'project-a' }, generated_at: at, blockers: [], blocked_work_items: [] };
async function worker(t: TestContext, respond: (req: http.IncomingMessage, res: http.ServerResponse) => void) {
  const server = http.createServer(respond);
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(async () => { server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); });
  return { node_id: 'worker-one', display_name: 'Worker One', advertised_url: `http://127.0.0.1:${(server.address() as { port: number }).port}`,
    version: '0.3.0', schema_digest: COORDINATOR_SCHEMA_DIGEST, transport_mode: 'loopback', secret_ref: 'env:UNUSED_FACTORY_TEST', profiles: ['project-a'] } as RegisteredNode;
}
const json = (res: http.ServerResponse, value: unknown) => { res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify(value)); };

test('worker factory reads fixed profile-scoped endpoints and projects bounded gates without credentials or commands', async t => {
  const paths: string[] = [];
  const node = await worker(t, (req, res) => {
    assert.equal(req.method, 'GET'); assert.equal(req.headers.authorization, 'Bearer synthetic-factory-token');
    const url = new URL(req.url!, 'http://fixture'); assert.equal(url.searchParams.get('profile'), 'project-a'); paths.push(url.pathname);
    if (url.pathname === '/api/status') json(res, { ...status, credential: 'PRIVATE_CANARY', blocked_work_items: Array.from({ length: 60 }, () => ({
      source_reference: '#42', reason_code: 'stuck_loop_gate', message: 'x'.repeat(2000),
      remediation_plan: { safe_actions: [{ summary: 'Inspect the current review evidence.', command: 'PRIVATE_CANARY', api_action: 'PRIVATE_CANARY' }] },
    })) });
    else if (url.pathname === '/api/loop/status') json(res, { running: false, credential: 'PRIVATE_CANARY' });
    else json(res, { timestamp: at, kind: 'human_required', reason: 'Review needs a decision.', work_id: '#42', reason_code: 'stuck_loop_gate', argv: 'PRIVATE_CANARY' });
  });
  const result = await readNodeFactory(node, 'project-a', { Authorization: 'Bearer synthetic-factory-token' });
  assert.deepEqual(paths.sort(), ['/api/loop/last-decision', '/api/loop/status', '/api/status']);
  assert.equal(result.node_id, node.node_id); assert.equal(result.profile, 'project-a');
  assert.deepEqual(result.loop, { state: 'available', value: { running: false } });
  assert.equal(result.status.state, 'available');
  if (result.status.state === 'available') {
    assert.equal(result.status.value.work_blocker_count, 60); assert.equal(result.status.value.work_blockers.length, 50);
    assert.equal(result.status.value.work_blockers[0].message?.length, 1024);
    assert.deepEqual(result.status.value.work_blockers[0].next_steps, ['Inspect the current review evidence.']);
  }
  assert.equal(JSON.stringify(result).includes('PRIVATE_CANARY'), false);
  assert.equal(JSON.stringify(result).includes('synthetic-factory-token'), false);
});

test('partial authorization and unsupported reads remain unknown while successful sources survive', async t => {
  const node = await worker(t, (req, res) => {
    if (req.url?.startsWith('/api/status?')) { res.statusCode = 403; res.end('PRIVATE_DENIAL_CANARY'); }
    else if (req.url?.startsWith('/api/loop/status?')) json(res, { running: true });
    else { res.statusCode = 404; res.end(); }
  });
  const result = await readNodeFactory(node, 'project-a', {});
  assert.deepEqual(result.status, { state: 'unavailable', reason: 'authorization' });
  assert.deepEqual(result.loop, { state: 'available', value: { running: true } });
  assert.deepEqual(result.decision, { state: 'unavailable', reason: 'unsupported' });
  assert.equal(JSON.stringify(result).includes('PRIVATE_DENIAL_CANARY'), false);
});

test('wrong-profile, oversized and malformed observations cannot become healthy or empty results', async t => {
  const node = await worker(t, (req, res) => {
    if (req.url?.startsWith('/api/status?')) json(res, { ...status, profile: { profile: 'other-project' } });
    else if (req.url?.startsWith('/api/loop/status?')) json(res, { running: true, padding: 'x'.repeat(2 * 1024 * 1024) });
    else res.end('{not-json');
  });
  const result = await readNodeFactory(node, 'project-a', {});
  for (const read of [result.status, result.loop, result.decision]) assert.deepEqual(read, { state: 'unavailable', reason: 'invalid_response' });
});

test('a body that never finishes shares the five-second deadline without losing independent reads', { timeout: 8000 }, async t => {
  const node = await worker(t, (req, res) => {
    if (req.url?.startsWith('/api/status?')) json(res, status);
    else if (req.url?.startsWith('/api/loop/status?')) { res.writeHead(200); res.write('{'); }
    else json(res, null);
  });
  const started = Date.now();
  const result = await readNodeFactory(node, 'project-a', {});
  assert.ok(Date.now() - started < 7000);
  assert.equal(result.status.state, 'available');
  assert.deepEqual(result.loop, { state: 'unavailable', reason: 'timeout' });
  assert.deepEqual(result.decision, { state: 'available', value: null });
});

test('redirects do not forward credentials and registry replacement invalidates a pending check', async t => {
  let redirected = 0;
  const target = await worker(t, (_req, res) => { redirected++; json(res, {}); });
  const redirect = await worker(t, (_req, res) => { res.writeHead(302, { Location: target.advertised_url }); res.end(); });
  const refused = await readNodeFactory(redirect, 'project-a', { Authorization: 'Bearer fixture' });
  assert.equal(refused.status.state, 'unavailable'); assert.equal(redirected, 0);
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  let seen!: () => void; const requested = new Promise<void>(resolve => { seen = resolve; });
  const node = await worker(t, (req, res) => { seen(); void gate.then(() => json(res, req.url?.startsWith('/api/status?') ? status : req.url?.startsWith('/api/loop/status?') ? { running: false } : null)); });
  const registry = new RegistryService(join(mkdtempSync(join(tmpdir(), 'factory-read-')), 'registry.json'));
  registry.registerNode(node);
  await assert.rejects(registry.checkNodeFactory(node.node_id, 'undeclared'), (e: unknown) => e instanceof NodeFactoryError && e.status === 400);
  await assert.rejects(registry.checkNodeFactory('missing', 'project-a'), (e: unknown) => e instanceof NodeFactoryError && e.status === 404);
  const pending = registry.checkNodeFactory(node.node_id, 'project-a');
  await requested;
  registry.registerNode({ ...node, display_name: 'Replacement registration' });
  release();
  await assert.rejects(pending, (e: unknown) => e instanceof NodeFactoryError && e.status === 409);
});

test('the central HTTP boundary exposes only the requested registered worker check and rejects untrusted reads', async t => {
  let reads = 0;
  const node = await worker(t, (req, res) => {
    reads++;
    json(res, req.url?.startsWith('/api/status?') ? status : req.url?.startsWith('/api/loop/status?') ? { running: false } : null);
  });
  const registry = new RegistryService(join(mkdtempSync(join(tmpdir(), 'factory-http-')), 'registry.json'));
  registry.registerNode(node);
  const server = http.createServer(createServer({ registryService: registry }));
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(async () => { server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve())); });
  const url = `http://127.0.0.1:${(server.address() as { port: number }).port}/api/registry/nodes/worker-one/factory?profile=project-a`;
  const denied = await fetch(url, { headers: { Origin: 'https://untrusted.invalid' } });
  assert.equal(denied.status, 403); assert.equal(reads, 0);
  const result = await fetch(url);
  assert.equal(result.status, 200); assert.equal(result.headers.get('cache-control'), 'no-store');
  const body = await result.json() as NodeFactorySnapshot;
  assert.equal(body.node_id, 'worker-one'); assert.equal(body.profile, 'project-a');
  assert.deepEqual(body.loop, { state: 'available', value: { running: false } });
  assert.equal(reads, 3);
  assert.equal((await fetch(url.replace('project-a', 'other-project'))).status, 400);
  assert.equal(reads, 3);
});
