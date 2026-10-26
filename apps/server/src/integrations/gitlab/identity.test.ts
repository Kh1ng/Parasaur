import assert from 'node:assert/strict';
import { test } from 'node:test';
import http from 'node:http';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { AddressInfo } from 'node:net';
import { WebSocket } from 'ws';
import { DeviceAccess } from '../../deviceAccess.js';
import { createServer } from '../../server.js';
import { createAuthorizedWebSocketServer, webSocketAccessValid, webSocketCanApprovePairing, webSocketGitLabPrincipal } from '../../webSocketAuth.js';
import { GITLAB_IDENTITY_HEADER, GitLabIdentity, anonymousGitLabIdentityProvider, type GitLabIdentityRequest, type GitLabPrincipal } from './identity.js';

test('default provider never authenticates, including forged identity headers', async () => {
  assert.equal(await anonymousGitLabIdentityProvider.authenticate({
    assertion: 'forged', method: 'GET', path: '/embed', host: 'panel.test', origin: undefined,
    forwardedPrefix: '/parasaur', remoteAddress: '127.0.0.1', secure: true,
  }), null);
  assert.equal(await new GitLabIdentity().authenticate({
    headers: { [GITLAB_IDENTITY_HEADER]: 'forged', 'x-gitlab-user': 'owner' }, socket: {},
  }, true), null);
});

test('without a provider, all 24 header/credential combinations match main on HTTP and WebSocket', async t => {
  // Captured by the reviewer's real-request probe against main at 6aedbe97.
  const baseline: { mode: string; headers: number; status: number; body: string; cookie: string | null; ws: number }[] =
    JSON.parse(readFileSync(new URL('../../../tests/fixtures/gitlab-identity/main-no-provider.json', import.meta.url), 'utf8'));
  assert.equal(baseline.length, 24);
  const directory = mkdtempSync(join(tmpdir(), 'gah-gitlab-no-provider-'));
  const keys = ['COORDINATOR_TOKEN', 'GAH_ALLOW_INSECURE_HTTP', 'GAH_COORDINATOR_IDENTITY_PATH', 'GAH_WS_AUTH_MODE', 'GAH_MUTATION_STORE_PATH'] as const;
  const saved = keys.map(key => process.env[key]);
  process.env.COORDINATOR_TOKEN = 'scratch-owner';
  process.env.GAH_ALLOW_INSECURE_HTTP = '1';
  process.env.GAH_COORDINATOR_IDENTITY_PATH = join(directory, 'identity.json');
  process.env.GAH_WS_AUTH_MODE = 'strict';
  process.env.GAH_MUTATION_STORE_PATH = join(directory, 'mutations');
  t.after(() => {
    keys.forEach((key, i) => saved[i] === undefined ? delete process.env[key] : process.env[key] = saved[i]);
    rmSync(directory, { recursive: true, force: true });
  });
  const access = new DeviceAccess(join(directory, 'devices.json'));
  const offer = access.create({ id: 'central', name: 'central', origin: 'https://panel.test' });
  const paired = access.redeem(offer.code, 'central', 'https://panel.test', 'scratch-device');
  const headerSets: Record<string, string>[] = [
    {}, { 'X-Gitlab-User': 'owner' }, { [GITLAB_IDENTITY_HEADER]: 'forged' },
    { [GITLAB_IDENTITY_HEADER]: '' }, { [GITLAB_IDENTITY_HEADER]: 'bad,duplicate' },
    { 'X-Gitlab-User': 'owner', [GITLAB_IDENTITY_HEADER]: 'forged' },
  ];
  // Cover both createServer's anonymous default and custom startup without a hook instance.
  for (const registration of ['default', 'absent']) {
    await t.test(registration, async t => {
      const app = createServer({ deviceAccess: access, detectTailscaleIPv4: async () => null });
      if (registration === 'absent') delete app.locals.gitLabIdentity;
      const server = http.createServer(app);
      const wss = createAuthorizedWebSocketServer(server, 'central', access, Date.now, app.locals.gitLabIdentity);
      t.after(async () => {
        for (const socket of wss.clients) socket.terminate();
        await new Promise<void>(resolve => wss.close(() => resolve()));
        server.closeAllConnections();
        await new Promise<void>(resolve => server.close(() => resolve()));
      });
      await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
      const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
      const connect = (headers: Record<string, string>) => new Promise<number>((resolve, reject) => {
        const socket = new WebSocket(base.replace('http:', 'ws:') + '/ws', ['gah.v1'], { headers });
        socket.once('open', () => { socket.terminate(); resolve(101); });
        socket.once('unexpected-response', (_req, response) => { response.resume(); resolve(response.statusCode!); });
        socket.once('error', reject);
      });
      for (const expected of baseline) {
        await t.test(`${expected.mode}, header set ${expected.headers}`, async () => {
          const headers: Record<string, string> = { Origin: base, ...headerSets[expected.headers] };
          if (expected.mode !== 'local') headers['X-Forwarded-Prefix'] = '/parasaur';
          if (expected.mode === 'owner') headers.Authorization = 'Bearer scratch-owner';
          if (expected.mode === 'device') headers.Cookie = `gah_device=${paired.token}`;
          const response = await fetch(base + '/api/pairing/session', { headers });
          assert.deepEqual({ ...expected, status: response.status,
            body: (await response.text()).replaceAll(paired.device.id, '<device-id>'),
            cookie: response.headers.get('set-cookie'), ws: await connect(headers) }, expected);
        });
      }
    });
  }
});

test('registered hook maps HTTP and WebSocket requests to bounded controller sessions and fails closed', async t => {
  const directory = mkdtempSync(join(tmpdir(), 'gah-gitlab-identity-'));
  const keys = ['COORDINATOR_TOKEN', 'GAH_ALLOW_INSECURE_HTTP', 'GAH_COORDINATOR_IDENTITY_PATH', 'GAH_WS_AUTH_MODE', 'GAH_MUTATION_STORE_PATH'] as const;
  const saved = keys.map(key => process.env[key]);
  process.env.COORDINATOR_TOKEN = 'owner-secret';
  process.env.GAH_ALLOW_INSECURE_HTTP = '1';
  process.env.GAH_COORDINATOR_IDENTITY_PATH = join(directory, 'identity.json');
  process.env.GAH_WS_AUTH_MODE = 'trusted_lan';
  process.env.GAH_MUTATION_STORE_PATH = join(directory, 'mutations');
  const access = new DeviceAccess(join(directory, 'devices.json'));
  const offer = access.create({ id: 'central', name: 'Central', origin: 'https://panel.test' });
  const { device, token } = access.redeem(offer.code, 'central', 'https://panel.test', 'GitLab user 42');
  let principal: GitLabPrincipal | null = { id: device.id, displayName: 'Alice', role: 'controller', expiresAt: device.expires_at };
  let throwing = false;
  const requests: GitLabIdentityRequest[] = [];
  const provider = { authenticate: async (request: GitLabIdentityRequest) => {
    requests.push(request);
    if (throwing) throw new Error('secret-provider-error');
    return request.assertion === 'verified.by.enterprise' ? principal : null;
  } };
  const savedWebRoot = process.env.GAH_WEB_ROOT;
  process.env.GAH_WEB_ROOT = directory;
  writeFileSync(join(directory, 'index.html'), '<html><head></head><body>panel</body></html>');
  t.after(() => {
    if (savedWebRoot === undefined) delete process.env.GAH_WEB_ROOT; else process.env.GAH_WEB_ROOT = savedWebRoot;
  });
  const app = createServer({ deviceAccess: access, gitLabIdentityProvider: provider, detectTailscaleIPv4: async () => null });
  const server = http.createServer(app);
  let now = Date.now();
  const wss = createAuthorizedWebSocketServer(server, 'central', access, () => now, app.locals.gitLabIdentity);
  let connected: WebSocket | undefined;
  wss.on('connection', ws => { connected = ws; });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  const headers = { Origin: base, 'X-Forwarded-Prefix': '/parasaur', [GITLAB_IDENTITY_HEADER]: 'verified.by.enterprise' };
  const session = (extra: Record<string, string> = {}) => fetch(base + '/api/pairing/session', { headers: { ...headers, ...extra } });
  const connect = (extra: Record<string, string> = {}): Promise<WebSocket | number> => new Promise((resolve, reject) => {
    const socket = new WebSocket(base.replace('http:', 'ws:') + '/ws', ['gah.v1'], { headers: { ...headers, ...extra } });
    socket.once('open', () => resolve(socket));
    socket.once('unexpected-response', (_req, res) => { res.resume(); resolve(res.statusCode!); });
    socket.once('error', reject);
  });
  t.after(async () => {
    for (const socket of wss.clients) socket.terminate();
    await new Promise<void>(resolve => wss.close(() => resolve()));
    server.closeAllConnections();
    await new Promise<void>(resolve => server.close(() => resolve()));
    keys.forEach((key, i) => saved[i] === undefined ? delete process.env[key] : process.env[key] = saved[i]);
    rmSync(directory, { recursive: true, force: true });
  });

  const response = await session({ Cookie: '_gitlab_session=never-forward' });
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { schema_version: 1,
    principal: { kind: 'device', ...principal }, can_approve_pairing: false });
  assert.equal(response.headers.get('set-cookie'), null, 'No longer-lived credential is minted');
  assert.deepEqual(requests[0], { assertion: 'verified.by.enterprise', method: 'GET', path: '/api/pairing/session',
    host: new URL(base).host, origin: base, forwardedPrefix: '/parasaur', remoteAddress: '127.0.0.1', secure: false });
  assert.ok(!JSON.stringify(requests).includes('never-forward'));
  assert.ok(!JSON.stringify(requests).includes('owner-secret'));
  // Documents and handoff setup/claim stay public; approval shares auth and remains owner-only.
  const publicCount = requests.length;
  assert.equal((await fetch(base + '/embed', { headers })).status, 200);
  const handoffHeaders = { ...headers, 'X-Forwarded-Proto': 'https', Origin: base.replace('http:', 'https:'), 'Content-Type': 'application/json' };
  const handoff = (path: string, assertion = 'verified.by.enterprise') => fetch(base + '/api/pairing/handoff' + path, {
    method: 'POST', headers: { ...handoffHeaders, [GITLAB_IDENTITY_HEADER]: assertion }, body: '{}',
  });
  assert.equal((await handoff('')).status, 200);
  assert.equal((await handoff('/claim')).status, 410);
  assert.equal(requests.length, publicCount, 'Public embed and handoff bootstrap do not authenticate');
  assert.equal((await handoff('/approve')).status, 403, 'Verified controller cannot approve owner-token handoff');
  assert.equal(requests.length, publicCount + 1);
  assert.equal(requests.at(-1)?.path, '/api/pairing/handoff/approve');
  assert.equal((await handoff('/approve', 'forged')).status, 401);
  assert.equal(requests.length, publicCount + 2, 'Handoff approval invokes the shared identity hook');
  // Existing explicit credentials win without invoking the provider, even for malformed assertions.
  const credentials: Record<string, string>[] = [{ Authorization: 'Bearer owner-secret' }, { Cookie: `gah_device=${token}` }];
  for (const assertion of ['verified.by.enterprise', 'forged', '', 'one,two']) {
    for (const credential of credentials) {
      const count = requests.length;
      const response = await session({ ...credential, [GITLAB_IDENTITY_HEADER]: assertion });
      assert.equal(response.status, 200);
      const body = await response.json();
      assert.deepEqual(body, { schema_version: 1, principal: 'Authorization' in credential
        ? { kind: 'owner' } : { kind: 'device', id: device.id }, can_approve_pairing: 'Authorization' in credential });
      const socket = await connect({ ...credential, [GITLAB_IDENTITY_HEADER]: assertion });
      assert.ok(socket instanceof WebSocket);
      assert.equal(webSocketGitLabPrincipal(connected!), undefined);
      assert.equal(requests.length, count, 'Explicit credentials must not reach the identity provider');
      socket.terminate();
    }
  }
  const invalidCredentials: Record<string, string>[] = [{ Authorization: 'Bearer wrong' }, { Cookie: 'gah_device=invalid' }];
  for (const credential of invalidCredentials) {
    const count = requests.length;
    assert.equal((await session(credential)).status, 401);
    assert.equal(await connect(credential), 401);
    assert.equal(requests.length, count, 'Invalid explicit credentials retain their existing rejection');
  }
  assert.equal((await fetch(base + '/api/pairing/devices', { headers })).status, 403, 'Controller cannot manage credentials');
  assert.equal((await fetch(base + '/api/dispatch', { method: 'POST', headers: { ...headers, 'Content-Type': 'application/json' }, body: '{}' })).status, 400,
    'Controller reaches existing dispatch validation');
  assert.equal((await fetch(base + '/api/route-approvals/grant', { method: 'POST', headers: { ...headers, 'Content-Type': 'application/json' }, body: '{}' })).status, 403,
    'Identity does not grant paid-route approval authority');
  const audit = readFileSync(join(directory, 'mutations/audit.jsonl'), 'utf8').trim().split('\n').map(line => JSON.parse(line));
  assert.equal(audit.at(-1).actor, `device:${device.id}`, 'Existing audit records retain the mapped actor');
  assert.equal((await session({ Origin: 'https://attacker.test' })).status, 401);

  // The local record bounds expiry even if enterprise returns a later date.
  principal = { ...principal!, expiresAt: '2099-01-01T00:00:00Z' };
  const bounded = await session();
  assert.equal((await bounded.json() as { principal: GitLabPrincipal }).principal.expiresAt, device.expires_at);
  const socket = await connect();
  assert.ok(socket instanceof WebSocket);
  assert.deepEqual(webSocketGitLabPrincipal(connected!), { kind: 'device', ...principal, expiresAt: device.expires_at });
  assert.equal(webSocketCanApprovePairing(connected!), false);
  socket.terminate();

  const valid = principal;
  for (const invalid of [
    null,
    { ...valid, id: 'unknown-principal' },
    { ...valid, expiresAt: new Date(Date.now() - 1).toISOString() },
    { ...valid, expiresAt: 'invalid' },
    { ...valid, displayName: '' },
    JSON.parse(JSON.stringify({ ...valid, role: 'owner' })),
    JSON.parse(JSON.stringify({ ...valid, role: 'future-role' })),
  ]) {
    principal = invalid;
    assert.equal((await session()).status, 401, 'Invalid identity cannot inherit local access');
    assert.equal(await connect(), 401, 'Invalid identity cannot enter trusted-LAN compatibility mode');
  }
  principal = valid;
  throwing = true;
  const denied = await session();
  assert.equal(denied.status, 401);
  assert.ok(!(await denied.text()).includes('secret-provider-error'));
  assert.equal(await connect(), 401);
  throwing = false;
  for (const assertion of ['forged', '', 'one,two', 'x'.repeat(8193)]) {
    assert.equal((await session({ [GITLAB_IDENTITY_HEADER]: assertion })).status, 401);
  }
  assert.equal((await fetch(base + '/api/pairing/session', { headers: { Origin: base, 'X-Forwarded-Prefix': '/parasaur', 'X-Gitlab-User': 'owner' } })).status, 401,
    'No assertion means no identity, even with a provider registered');

  const workerWss = createAuthorizedWebSocketServer(http.createServer(), 'worker', access, Date.now, app.locals.gitLabIdentity);
  try {
    Reflect.apply(workerWss.options.verifyClient!, workerWss, [{ req: {
      headers: { ...headers, host: new URL(base).host, origin: base }, socket: { remoteAddress: '127.0.0.1' }, method: 'GET', url: '/ws',
    } }, (allowed: boolean, status: number) => { assert.equal(allowed, false); assert.equal(status, 401); }]);
  } finally { workerWss.close(); }

  principal = { ...valid!, expiresAt: new Date(now + 60_000).toISOString() };
  const expiring = await connect();
  assert.ok(expiring instanceof WebSocket);
  now += 60_001;
  assert.equal(webSocketAccessValid(connected!), false, 'Buffered messages cannot outlive the assertion');
  assert.equal(webSocketGitLabPrincipal(connected!), undefined);
  expiring.terminate();
  principal = valid;
  const revoked = await connect();
  assert.ok(revoked instanceof WebSocket);
  access.revoke(device.id);
  assert.equal(webSocketAccessValid(connected!), false);
  assert.equal((await session()).status, 401);
  assert.equal(await connect(), 401);
  revoked.terminate();
});
