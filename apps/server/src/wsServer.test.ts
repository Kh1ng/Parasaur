import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { test } from 'node:test';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WebSocket, WebSocketServer } from 'ws';
import { RegistryService } from './registryService.js';
import { COORDINATOR_SCHEMA_DIGEST, getCoordinatorIdentity } from './coordinatorIdentity.js';
import { createWebSocketHandler } from './wsServer.js';
import { createAuthorizedWebSocketServer } from './webSocketAuth.js';
import { runEvents } from './gahCli.js';
import type { ControllerEvent, NodeRoleStatus, ServerMessage } from '@git-agent-harness/contracts';
import { ActivityFeed, activityFromController } from './activityFeed.js';

// Hermetic: welcome is pushed only after gahCli.runStatus() spawns a real
// `gah status` child. A cold call of the repo's release binary takes ~21s
// here (statusCache then serves subsequent calls), which blows past the
// per-message welcome deadline and makes the first connection a flaky
// ~20s timeout. Point GAH_BINARY at the instant fixture instead.
process.env.GAH_BINARY = fileURLToPath(new URL('../tests/fixtures/gah/gah', import.meta.url));

/**
 * AC3 ("a restored WebSocket connection re-triggers a fresh REST pull",
 * apps/web's useWsReconnectRefresh/reconnectSeq) assumes every new socket
 * connection to the real server -- not just the first one -- gets a fresh
 * server.welcome push, independent of any prior connection's lifecycle.
 * The web e2e spec (dashboard-freshness.spec.ts) only proves the client
 * reacts correctly to whatever a hand-authored Playwright WS mock decides
 * to send; it never touches this file. This exercises the actual
 * production handler (wss.on('connection', ...) -> sendWelcomeMessage)
 * against real `ws` sockets to ground that assumption in the real
 * server/client boundary.
 */
async function withWsServer(testFn: (wsUrl: string) => Promise<void>, node?: NodeRoleStatus) {
  const server = http.createServer();
  const wss = createAuthorizedWebSocketServer(server, node?.role ?? 'central');
  createWebSocketHandler(wss, { node });

  await new Promise<void>((resolve) => server.listen(0, resolve));
  const { port } = server.address() as AddressInfo;

  try {
    await testFn(`ws://127.0.0.1:${port}`);
  } finally {
    wss.close();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}

// sendWelcomeMessage awaits a real `gah status --json` child process
// (gahCli.runStatus) before pushing -- typically single-digit seconds on
// first call, then cached (see gahCli.ts's statusCache) for the rest of
// the run. This margin is for that first, uncached call.
function nextWelcome(ws: WebSocket): Promise<ServerMessage & { type: 'server.welcome' }> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('timed out waiting for server.welcome')), 15000);
    ws.on('message', (data) => {
      const message = JSON.parse(data.toString()) as ServerMessage;
      if (message.type === 'server.welcome') {
        clearTimeout(timer);
        resolve(message);
      }
    });
    ws.on('error', (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

async function connectAndAwaitWelcome(wsUrl: string, token?: string): Promise<WebSocket> {
  const ws = new WebSocket(wsUrl, { headers: token ? { Authorization: `Bearer ${token}` } : {} });
  await new Promise<void>((resolve, reject) => {
    ws.on('open', resolve);
    ws.on('error', reject);
  });
  await nextWelcome(ws);
  return ws;
}

test(
  'a fresh connection after a prior one closed still gets its own server.welcome (real handler, real sockets)',
  { timeout: 20000 },
  async () => {
    await withWsServer(async (wsUrl) => {
      const first = await connectAndAwaitWelcome(wsUrl);
      first.close();
      await new Promise<void>((resolve) => first.on('close', () => resolve()));

      // Simulates the client-side reconnect in WebSocketContext.tsx: a brand
      // new socket opened after the previous one dropped. If the real server
      // only pushed state on the very first connection ever, this would hang
      // and nextWelcome's own guard above would time this out.
      const second = await connectAndAwaitWelcome(wsUrl);
      second.close();
    });
  }
);

test(
  'two independent connections each receive their own independent server.welcome',
  { timeout: 20000 },
  async () => {
    await withWsServer(async (wsUrl) => {
      const a = await connectAndAwaitWelcome(wsUrl);
      const b = await connectAndAwaitWelcome(wsUrl);
      assert.notEqual(a, b);
      a.close();
      b.close();
    });
  }
);

test('server.welcome reconciles persisted leases before reporting running sessions', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-welcome-leases-'));
  const leasesPath = join(dir, 'dispatch-leases.json');
  const identity = getCoordinatorIdentity();
  const timestamp = new Date().toISOString();
  const staleLease = {
    requestId: 'stale-request',
    workKey: 'stale-work',
    profile: 'gah',
    nodeId: identity.node_id,
    nodeUrl: identity.advertised_url,
    session: {
      id: 'stale-session',
      providerKind: 'codex',
      instanceId: 'codex-0',
      status: 'running',
      repo: 'owner/repo',
      mode: 'improve'
    },
    state: 'running',
    createdAt: timestamp,
    updatedAt: timestamp,
    coordinatorNodeId: identity.node_id
  };
  writeFileSync(leasesPath, JSON.stringify({
    leases: [
      staleLease,
      {
        ...staleLease,
        requestId: 'other-request',
        workKey: 'other-work',
        profile: 'other',
        session: { ...staleLease.session, id: 'other-session' }
      }
    ]
  }));

  const previousPath = process.env.GAH_DISPATCH_LEASES_PATH;
  process.env.GAH_DISPATCH_LEASES_PATH = leasesPath;
  try {
    await withWsServer(async (wsUrl) => {
      const ws = new WebSocket(wsUrl);
      try {
        const welcome = await nextWelcome(ws);
        assert.deepEqual(welcome.sessions, []);
      } finally {
        ws.close();
      }
    });
  } finally {
    if (previousPath === undefined) delete process.env.GAH_DISPATCH_LEASES_PATH;
    else process.env.GAH_DISPATCH_LEASES_PATH = previousPath;
    rmSync(dir, { recursive: true });
  }
});


test('registry changes push only an invalidation over the existing websocket', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-fleet-push-'));
  const registry = new RegistryService(join(dir, 'registry.json'));
  const server = http.createServer();
  const wss = new WebSocketServer({ server });
  createWebSocketHandler(wss, { registryService: registry });
  const hello = new Promise<void>((resolve) => wss.once('connection', (socket) => socket.once('message', () => resolve())));
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const ws = new WebSocket(`ws://127.0.0.1:${(server.address() as AddressInfo).port}`);
  try {
    ws.once('open', () => ws.send(JSON.stringify({ type: 'client.hello', clientVersion: 'test', capabilities: {} })));
    await hello;
    const changed = new Promise<unknown>((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('No fleet invalidation received')), 2000);
      ws.on('message', (bytes) => {
        const message = JSON.parse(bytes.toString());
        if (message.type === 'fleet.changed') { clearTimeout(timer); resolve(message); }
      });
    });
    registry.registerNode({ node_id: 'hidden-node-id', display_name: 'Fleet Worker', advertised_url: 'http://127.0.0.1:9',
      version: '0.1.0', schema_digest: COORDINATOR_SCHEMA_DIGEST, transport_mode: 'loopback', secret_ref: 'env:PRIVATE_CANARY' });
    assert.deepEqual(await changed, { type: 'fleet.changed' });
  } finally {
    ws.terminate();
    for (const socket of wss.clients) socket.terminate();
    await new Promise<void>((resolve) => wss.close(() => resolve()));
    await new Promise<void>((resolve) => server.close(() => resolve()));
    rmSync(dir, { recursive: true });
  }
});

test('activity reconnect replays only events after the client cursor', async () => {
  const server = http.createServer();
  const wss = new WebSocketServer({ server });
  const events: ControllerEvent[] = [{
    timestamp: '2026-09-12T12:00:00.000Z', event_type: 'dispatch_finished', profile: 'gah',
    work_id: '#941', details: 'dispatch_ticket: success'
  }];
  createWebSocketHandler(wss, {
    activityFeed: new ActivityFeed(null),
    runEvents: async () => events
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const url = `ws://127.0.0.1:${(server.address() as AddressInfo).port}`;
  const connect = async (cursor?: string) => {
    const ws = new WebSocket(url);
    await new Promise<void>((resolve) => ws.once('open', resolve));
    const replay = new Promise<Extract<ServerMessage, { type: 'activity.replay' }>>((resolve) => {
      ws.on('message', (bytes) => {
        const message = JSON.parse(bytes.toString()) as ServerMessage;
        if (message.type === 'activity.replay') resolve(message);
      });
    });
    ws.send(JSON.stringify({ type: 'client.hello', clientVersion: 'test', profile: 'gah', activityCursor: cursor, capabilities: {} }));
    return { ws, replay: await replay };
  };
  try {
    const first = await connect();
    assert.equal(first.replay.events[0]?.kind, 'dispatch_completed');
    const cursor = activityFromController(events[0])!.id;
    first.ws.close();
    await new Promise<void>((resolve) => first.ws.once('close', resolve));
    events.push({
      timestamp: '2026-09-12T12:01:00.000Z', event_type: 'dispatch_finished', profile: 'gah',
      work_id: '#942', details: 'review_mr: success'
    });
    const second = await connect(cursor);
    assert.deepEqual(second.replay.events.map((event) => event.kind), ['review_ready']);
    second.ws.close();
  } finally {
    for (const socket of wss.clients) socket.terminate();
    await new Promise<void>((resolve) => wss.close(() => resolve()));
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});

test('background activity is delivered without a dashboard and backfill stays silent', async () => {
  const server = http.createServer();
  const wss = new WebSocketServer({ server });
  const events: ControllerEvent[] = [{
    timestamp: '2026-09-12T12:00:00.000Z', event_type: 'dispatch_finished', profile: 'gah',
    work_id: '#941', details: 'dispatch_ticket: success'
  }];
  const delivered: string[] = [];
  let loads = 0;
  createWebSocketHandler(wss, {
    activityFeed: new ActivityFeed(null, (event) => { delivered.push(event.id); }),
    runEvents: async () => { loads += 1; return [...events]; },
    runQuota: (async () => { throw new Error('quota unavailable'); }) as never,
    backgroundProfiles: async () => ['gah'],
    backgroundIntervalMs: 10
  });
  const waitForLoads = async (count: number) => {
    const deadline = Date.now() + 2000;
    while (loads < count) {
      if (Date.now() > deadline) throw new Error(`timed out waiting for ${count} activity loads`);
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  };
  try {
    await waitForLoads(2);
    assert.deepEqual(delivered, [], 'history loaded at startup must not be pushed');
    events.push({
      timestamp: '2026-09-12T12:01:00.000Z', event_type: 'dispatch_finished', profile: 'gah',
      work_id: '#942', details: 'review_mr: success'
    });
    await waitForLoads(loads + 2);
    assert.deepEqual(delivered, [activityFromController(events[1])!.id]);
  } finally {
    await new Promise<void>((resolve) => wss.close(() => resolve()));
  }
});

test('worker websocket authenticates execution handshake and rejects manager chat', { timeout: 5000 }, async () => {
  const previousToken = process.env.COORDINATOR_TOKEN;
  process.env.COORDINATOR_TOKEN = 'worker-integration-token';
  try { await withWsServer(async (wsUrl) => {
    await assert.rejects(connectAndAwaitWelcome(wsUrl), /401/);
    const ws = await connectAndAwaitWelcome(wsUrl, 'worker-integration-token');
    try {
      const rejected = new Promise<{ type: string; error: string }>((resolve) => ws.once('message', (data) => resolve(JSON.parse(data.toString()))));
      ws.send(JSON.stringify({ type: 'manager.chat.sessionList', profile: 'gah' }));
      const response = await rejected;
      assert.equal(response.type, 'error');
      assert.match(response.error, /only available on the central node/);
    } finally { ws.close(); }
  }, { role: 'worker', central_url: 'https://central.test' });
  } finally {
    if (previousToken === undefined) delete process.env.COORDINATOR_TOKEN; else process.env.COORDINATOR_TOKEN = previousToken;
  }
});

test('trusted-LAN sockets receive a warning but cannot invoke session mutations', async (t) => {
  const { getFleetDispatch } = await import('./wsServer.js');
  const savedMode = process.env.GAH_WS_AUTH_MODE;
  const savedHttp = process.env.GAH_ALLOW_INSECURE_HTTP;
  process.env.GAH_WS_AUTH_MODE = 'trusted_lan';
  process.env.GAH_ALLOW_INSECURE_HTTP = '1';
  const server = http.createServer();
  const wss = createAuthorizedWebSocketServer(server, 'central');
  createWebSocketHandler(wss);
  const fleet = getFleetDispatch();
  const start = t.mock.method(fleet, 'startSession', async () => { throw new Error('must not start'); });
  const stop = t.mock.method(fleet, 'stopSession', async () => { throw new Error('must not stop'); });
  const send = t.mock.method(fleet, 'sendCommand', async () => { throw new Error('must not send'); });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = (server.address() as AddressInfo).port;
  const ws = new WebSocket(`ws://127.0.0.1:${port}/ws`, { headers: { host: `192.168.1.25:${port}`, origin: `http://192.168.1.25:${port}` } });
  try {
    const welcome = await nextWelcome(ws);
    assert.equal(welcome.trustedLanMode, true);
    for (const type of ['session.start', 'session.stop', 'session.sendCommand']) {
      const reply = new Promise<ServerMessage>(resolve => ws.once('message', data => resolve(JSON.parse(data.toString()))));
      ws.send(JSON.stringify({ type, requestId: type, profile: 'gah', nodeId: 'remote', coordinatorNodeId: 'spoof', sessionId: 'remote-session', command: 'touch forbidden' }));
      const response = await reply;
      assert.equal(response.type, 'error');
      assert.match(JSON.stringify(response), /coordinator token is required/);
    }
    assert.equal(start.mock.callCount(), 0);
    assert.equal(stop.mock.callCount(), 0);
    assert.equal(send.mock.callCount(), 0);
  } finally {
    ws.terminate();
    await new Promise<void>(resolve => wss.close(() => resolve()));
    await new Promise<void>(resolve => server.close(() => resolve()));
    if (savedMode === undefined) delete process.env.GAH_WS_AUTH_MODE; else process.env.GAH_WS_AUTH_MODE = savedMode;
    if (savedHttp === undefined) delete process.env.GAH_ALLOW_INSECURE_HTTP; else process.env.GAH_ALLOW_INSECURE_HTTP = savedHttp;
  }
});

/** Fake `gah` for #1598: the 30-day `events` window holds 250 old
 * `context_built` events with ~280 KB payloads (~70 MB, over the server's
 * events output cap) followed by 200 ordinary dispatch events, so the
 * unlimited window read is over cap exactly like production. `--limit N`
 * returns only the newest N events, mirroring the real CLI. `status` and
 * `quota` replay the committed fixture responses so the rest of the
 * refresh path behaves normally. */
function writeOverCapEventsGah(dir: string): string {
  const responses = fileURLToPath(new URL('../tests/fixtures/gah/responses', import.meta.url));
  const bin = join(dir, 'gah');
  writeFileSync(bin, `#!/usr/bin/env node
import fs from 'node:fs';

const args = process.argv.slice(2);
const command = args[0];
if (command === 'status') {
  process.stdout.write(fs.readFileSync('${join(responses, 'status.json')}', 'utf8'));
  process.exit(0);
}
if (command === 'quota') {
  process.stdout.write(fs.readFileSync('${join(responses, 'quota.json')}', 'utf8'));
  process.exit(0);
}
if (command !== 'events') process.exit(0);

const newest = [];
for (let i = 0; i < 200; i++) {
  newest.push({
    timestamp: new Date(Date.now() - (200 - i) * 60000).toISOString(),
    event_type: 'dispatch_finished',
    profile: 'overcap',
    work_id: '#new-' + i,
    details: 'dispatch_ticket: success'
  });
}
const limitIndex = args.indexOf('--limit');
if (limitIndex !== -1) {
  const limit = Number(args[limitIndex + 1]);
  process.stdout.write(JSON.stringify(newest.slice(-limit)));
  process.exit(0);
}
const giant = JSON.stringify({
  timestamp: '2026-10-10T00:00:00.000Z',
  event_type: 'context_built',
  profile: 'overcap',
  work_id: '#giant',
  details: 'context: ' + 'x'.repeat(280000)
});
process.stdout.write('[');
for (let i = 0; i < 250; i++) {
  process.stdout.write((i ? ',' : '') + giant);
}
process.stdout.write(',' + newest.map((event) => JSON.stringify(event)).join(',') + ']');
`);
  chmodSync(bin, 0o755);
  return bin;
}

test('an over-cap event window still refreshes the feed with the newest events (#1598)', { timeout: 20000 }, async () => {
  const fakeDir = mkdtempSync(join(tmpdir(), 'gah-overcap-events-'));
  const fakeBin = writeOverCapEventsGah(fakeDir);
  const previousBinary = process.env.GAH_BINARY;
  process.env.GAH_BINARY = fakeBin;
  try {
    // The unlimited 30-day window read (what the HTTP endpoints request)
    // exceeds the events output cap: the incident, reproduced through the
    // real spawn/parse transport.
    await assert.rejects(
      runEvents('overcap', '30d'),
      (error: unknown) => error instanceof Error && /gah events output exceeded \d+ bytes/.test(error.message)
    );

    const server = http.createServer();
    const wss = new WebSocketServer({ server });
    createWebSocketHandler(wss, { activityFeed: new ActivityFeed(null) });
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
    const ws = new WebSocket(`ws://127.0.0.1:${(server.address() as AddressInfo).port}`);
    try {
      // client.hello awaits the profile's activity sync and then replays the
      // feed: if the bounded refresh did not deliver, this replay is empty.
      const replay = new Promise<Extract<ServerMessage, { type: 'activity.replay' }>>((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('timed out waiting for activity.replay')), 15000);
        ws.on('message', (bytes) => {
          const message = JSON.parse(bytes.toString()) as ServerMessage;
          if (message.type === 'activity.replay') { clearTimeout(timer); resolve(message); }
        });
      });
      await new Promise<void>((resolve) => ws.once('open', resolve));
      ws.send(JSON.stringify({ type: 'client.hello', clientVersion: 'test', profile: 'overcap', capabilities: {} }));
      const message = await replay;
      const delivered = message.events.filter((event) => event.workId !== null && event.workId !== undefined);
      assert.ok(delivered.length >= 199, `the refresh must deliver the newest window, got ${delivered.length} events`);
      assert.ok(delivered.some((event) => event.workId === '#new-199'), 'the newest event must be delivered');
      assert.ok(
        delivered.every((event) => /^#new-\d+$/.test(event.workId ?? '')),
        'only the newest window events may be delivered'
      );
    } finally {
      ws.terminate();
      for (const socket of wss.clients) socket.terminate();
      await new Promise<void>((resolve) => wss.close(() => resolve()));
      await new Promise<void>((resolve) => server.close(() => resolve()));
    }
  } finally {
    if (previousBinary === undefined) delete process.env.GAH_BINARY; else process.env.GAH_BINARY = previousBinary;
    rmSync(fakeDir, { recursive: true, force: true });
  }
});

test('a failing events refresh logs one warning per transition, not one per poll (#1598)', async (t) => {
  const server = http.createServer();
  const wss = new WebSocketServer({ server });
  let failEvents = true;
  let loads = 0;
  let errorMessage = 'gah events output exceeded 67108864 bytes (observed 67108966)';
  createWebSocketHandler(wss, {
    activityFeed: new ActivityFeed(null),
    runEvents: async () => {
      loads += 1;
      if (failEvents) throw new Error(errorMessage);
      return [];
    },
    runQuota: (async () => { throw new Error('quota unavailable'); }) as never,
    backgroundProfiles: async () => ['gah'],
    backgroundIntervalMs: 10
  });
  const failureLogs = t.mock.method(console, 'error');
  const recoveryLogs = t.mock.method(console, 'log');
  const waitForLoads = async (count: number) => {
    const deadline = Date.now() + 2000;
    while (loads < count) {
      if (Date.now() > deadline) throw new Error(`timed out waiting for ${count} activity loads`);
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    await new Promise((resolve) => setTimeout(resolve, 5));
  };
  const activityFailures = () => failureLogs.mock.calls
    .filter((call) => typeof call.arguments[0] === 'string' && call.arguments[0].includes('Failed to refresh activity for gah')).length;
  const recoveries = () => recoveryLogs.mock.calls
    .filter((call) => typeof call.arguments[0] === 'string' && call.arguments[0].includes('Activity refresh for gah recovered')).length;
  try {
    await waitForLoads(3);
    assert.equal(activityFailures(), 1, 'three failed polls with the same message must log the failure exactly once');
    failEvents = false;
    await waitForLoads(loads + 2);
    assert.equal(recoveries(), 1, 'a successful refresh after failures must log the recovery exactly once');
    failEvents = true;
    await waitForLoads(loads + 2);
    assert.equal(activityFailures(), 2, 'a second failing transition must log the failure again');
    // #1616: the once-per-transition suppression must be keyed by the error
    // message too -- a profile-only set suppressed every NEW error behind
    // the first one until the profile happened to recover. A distinct
    // message must be visible even while the profile never succeeds, while
    // the same message repeating stays quiet.
    errorMessage = 'authentication_error: credential expired';
    await waitForLoads(loads + 2);
    assert.equal(activityFailures(), 3, 'a distinct new error message must log again without an intervening recovery');
    await waitForLoads(loads + 2);
    assert.equal(activityFailures(), 3, 'the same message repeating must stay quiet');
    errorMessage = 'gah events output exceeded 67108864 bytes (observed 67108966)';
    await waitForLoads(loads + 2);
    assert.equal(activityFailures(), 4, 'switching back to the first message is another change and must log again');
  } finally {
    failureLogs.mock.restore();
    recoveryLogs.mock.restore();
    await new Promise<void>((resolve) => wss.close(() => resolve()));
  }
});
