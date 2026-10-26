import assert from 'node:assert/strict';
import { test } from 'node:test';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import express from 'express';
import { WebSocket } from 'ws';
import { authMiddleware, requireOwner } from './authMiddleware.js';
import { mutationSafety } from './mutationSafety.js';
import { panelIdentityMiddleware, type PanelIdentityProvider } from './panelIdentity.js';
import { attributePanelMessage, createAuthorizedWebSocketServer, webSocketCanApprovePairing } from './webSocketAuth.js';

// Simulates a verified server-side session, never identity headers.
test('panel identity is verified, attributed and restricted independently of owner/device access', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'panel-identity-'));
  const saved = process.env.GAH_MUTATION_STORE_PATH;
  process.env.GAH_MUTATION_STORE_PATH = directory;
  let subject: string | null = 'gitlab-user-42';
  let approve = false;
  let broken = false;
  const provider: PanelIdentityProvider = { async authenticate() {
    if (broken) throw new Error('verification unavailable');
    return subject === null ? null : { issuer: 'https://gitlab.example', subject, canApprovePaidRoutes: approve };
  } };
  const app = express();
  app.use(express.json(), panelIdentityMiddleware(provider), authMiddleware);
  const mutation = mutationSafety('central', directory);
  let actions = 0;
  app.post('/api/dispatch', (_req, res) => { actions++; res.json({ ok: true }); });
  app.post('/api/manager-chat/sessions', (_req, res) => res.json({ ok: true }));
  app.post('/api/approve', mutation('route_approval.grant'), (_req, res) => { actions++; res.json({ ok: true }); });
  app.post('/api/admin', requireOwner, (_req, res) => res.json({ ok: true }));
  const server = http.createServer(app);
  const wss = createAuthorizedWebSocketServer(server, 'central', undefined, Date.now, provider);
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = (server.address() as AddressInfo).port;
  const post = (path: string) => fetch(`http://127.0.0.1:${port}/api/${path}`, { method: 'POST', headers: { 'Content-Type': 'application/json', 'Idempotency-Key': 'same-key-for-each-user', 'X-Gitlab-User': 'forged-admin' }, body: JSON.stringify({ actor: 'forged-admin' }) });
  let client: WebSocket | undefined;
  try {
    assert.equal((await post('dispatch')).status, 200);
    assert.equal((await post('manager-chat/sessions')).status, 200);
    assert.equal((await post('approve')).status, 403);
    assert.equal((await post('admin')).status, 403);
    approve = true;
    assert.equal((await post('approve')).status, 200);
    assert.equal((await post('approve')).status, 409);
    subject = 'gitlab-user-43';
    assert.equal((await post('approve')).status, 200);
    client = await new Promise<WebSocket>((resolve, reject) => {
      const ws = new WebSocket(`ws://127.0.0.1:${port}`);
      ws.once('open', () => resolve(ws)); ws.once('error', reject);
    });
    const socket = [...wss.clients][0];
    assert.equal(webSocketCanApprovePairing(socket), false);
    await attributePanelMessage(socket, 'manager.chat.send', 'chat-turn-123', { text: 'private chat' });
    subject = null;
    await assert.rejects(attributePanelMessage(socket, 'manager.chat.send', 'expired'));
    assert.equal((await post('dispatch')).status, 401);
    broken = true;
    assert.equal((await post('dispatch')).status, 503);
    assert.equal(actions, 3);
    const audit = readFileSync(join(directory, 'audit.jsonl'), 'utf8');
    assert.match(audit, /gitlab-user-42/); assert.match(audit, /gitlab-user-43/);
    const attribution = readFileSync(join(directory, 'panel-actions.jsonl'), 'utf8');
    assert.match(attribution, /POST \/api\/dispatch/); assert.match(attribution, /manager.chat.send/);
    assert.match(attribution, /chat-turn-123/);
    assert.doesNotMatch(audit + attribution, /forged-admin|private chat/);
  } finally {
    client?.terminate();
    for (const socket of wss.clients) socket.terminate();
    await new Promise<void>(resolve => wss.close(() => resolve()));
    await new Promise<void>(resolve => server.close(() => resolve()));
    if (saved === undefined) delete process.env.GAH_MUTATION_STORE_PATH; else process.env.GAH_MUTATION_STORE_PATH = saved;
    rmSync(directory, { recursive: true, force: true });
  }
});

test('unconfigured identity hook ignores forged GitLab headers', async () => {
  const app = express();
  app.use(panelIdentityMiddleware(), authMiddleware);
  app.get('/', (_req, res) => res.json(res.locals.authPrincipal));
  const server = app.listen(0, '127.0.0.1');
  await new Promise<void>(resolve => server.once('listening', resolve));
  try {
    const response = await fetch(`http://127.0.0.1:${(server.address() as AddressInfo).port}/`, { headers: { 'X-Gitlab-User': 'admin' } });
    assert.deepEqual(await response.json(), { kind: 'owner' });
  } finally { await new Promise<void>(resolve => server.close(() => resolve())); }
});

test('identity validation, origin checks and durable attribution fail closed without a listener', async () => {
  const { resolvePanelIdentity, panelActor, recordPanelAction } = await import('./panelIdentity.js');
  const request = { headers: {}, method: 'POST', path: '/dispatch', body: { message: 'private' } } as unknown as express.Request;
  const identity = { issuer: 'gitlab', subject: '42', canApprovePaidRoutes: false };
  assert.notEqual(panelActor(identity), panelActor({ ...identity, issuer: 'other' }));
  await assert.rejects(resolvePanelIdentity({ async authenticate() { return { ...identity, subject: '' }; } }, request));
  await assert.rejects(resolvePanelIdentity({ async authenticate() { return { ...identity, canApprovePaidRoutes: undefined } as never; } }, request));
  const directory = mkdtempSync(join(tmpdir(), 'panel-unit-'));
  const saved = process.env.GAH_MUTATION_STORE_PATH;
  process.env.GAH_MUTATION_STORE_PATH = directory;
  try {
    const res = { locals: {}, statusCode: 200, setHeader() {}, status(this: { statusCode: number }, code: number) { this.statusCode = code; return this; }, json(body: unknown) { return body; } } as unknown as express.Response;
    let next = false;
    await panelIdentityMiddleware({ async authenticate() { return identity; } })(request, res, () => { next = true; });
    assert.equal(next, true);
    assert.equal(res.locals.authPrincipal.identity.subject, '42');
    assert.doesNotMatch(readFileSync(join(directory, 'panel-actions.jsonl'), 'utf8'), /private/);
    next = false;
    await panelIdentityMiddleware({ async authenticate() { return null; } })(request, res, () => { next = true; });
    assert.equal(res.statusCode, 401); assert.equal(next, false);
    request.headers.cookie = 'session=forged';
    await panelIdentityMiddleware({ async authenticate() { throw new Error('should not run'); } })(request, res, () => { next = true; });
    assert.equal(res.statusCode, 403);
    process.env.GAH_MUTATION_STORE_PATH = join(directory, 'panel-actions.jsonl', 'impossible');
    assert.throws(() => recordPanelAction(identity, 'dispatch'));
    delete request.headers.cookie;
    await panelIdentityMiddleware({ async authenticate() { return identity; } })(request, res, () => { next = true; });
    assert.equal(res.statusCode, 503); assert.equal(next, false);
  } finally {
    if (saved === undefined) delete process.env.GAH_MUTATION_STORE_PATH; else process.env.GAH_MUTATION_STORE_PATH = saved;
    rmSync(directory, { recursive: true, force: true });
  }
});

test('paid-route policy and replay receipts distinguish verified users without a listener', () => {
  const directory = mkdtempSync(join(tmpdir(), 'panel-policy-'));
  const mutation = mutationSafety('central', directory);
  const request = { params: {}, body: { work_id: '#1596' }, query: {}, get() { return 'replay-key-for-panel-user'; } } as unknown as express.Request;
  const run = (subject: string, canApprovePaidRoutes: boolean, operation: Parameters<typeof mutation>[0] = 'route_approval.grant') => {
    let accepted = false;
    const res = { locals: { authPrincipal: { kind: 'panel', identity: { issuer: 'gitlab', subject, canApprovePaidRoutes } } }, statusCode: 200, setHeader() {}, status(this: { statusCode: number }, code: number) { this.statusCode = code; return this; }, json(body: unknown) { return body; } } as unknown as express.Response;
    mutation(operation)(request, res, () => { accepted = true; res.json({ ok: true }); });
    return { accepted, status: res.statusCode };
  };
  try {
    assert.deepEqual(run('42', false), { accepted: false, status: 403 });
    assert.deepEqual(run('42', true), { accepted: true, status: 200 });
    assert.deepEqual(run('42', true), { accepted: false, status: 409 });
    assert.deepEqual(run('43', true), { accepted: true, status: 200 });
    assert.deepEqual(run('42', true, 'config.prompt_policy.set'), { accepted: false, status: 403 });
    assert.deepEqual(run('42', true, 'route_approval.revoke'), { accepted: true, status: 200 });
    const audit = readFileSync(join(directory, 'audit.jsonl'), 'utf8');
    assert.match(audit, /42/); assert.match(audit, /43/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test('WebSocket attribution revalidates identity before each message without a listener', async () => {
  const { EventEmitter } = await import('node:events');
  const directory = mkdtempSync(join(tmpdir(), 'panel-ws-unit-'));
  const saved = process.env.GAH_MUTATION_STORE_PATH;
  process.env.GAH_MUTATION_STORE_PATH = directory;
  let active = true;
  let terminated = false;
  const server = http.createServer();
  const wss = createAuthorizedWebSocketServer(server, 'central', undefined, Date.now, {
    async authenticate() { return active ? { issuer: 'gitlab', subject: '42', canApprovePaidRoutes: true } : null; },
  });
  const socket = Object.assign(new EventEmitter(), { terminate() { terminated = true; } }) as unknown as WebSocket;
  try {
    wss.emit('connection', socket, { headers: {} });
    assert.equal(webSocketCanApprovePairing(socket), false);
    await attributePanelMessage(socket, 'session.start', 'dispatch-42', { actor: 'forged' });
    await attributePanelMessage(socket, 'manager.chat.send', 'chat-42', { text: 'private text' });
    active = false;
    await assert.rejects(attributePanelMessage(socket, 'manager.chat.send', 'expired'));
    assert.equal(terminated, true);
    const artifact = readFileSync(join(directory, 'panel-actions.jsonl'), 'utf8');
    assert.match(artifact, /session.start/); assert.match(artifact, /chat-42/);
    assert.doesNotMatch(artifact, /forged|private text|expired/);
  } finally {
    wss.close();
    if (saved === undefined) delete process.env.GAH_MUTATION_STORE_PATH; else process.env.GAH_MUTATION_STORE_PATH = saved;
    rmSync(directory, { recursive: true, force: true });
  }
});
