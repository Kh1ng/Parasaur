import assert from 'node:assert/strict';
import { test } from 'node:test';
import http from 'node:http';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import type { AddressInfo } from 'node:net';
import type { PairingAccessRequest } from '@git-agent-harness/contracts';
import { DeviceAccess } from './deviceAccess.js';
import { createServer } from './server.js';
import { resetCachedCoordinatorIdentity } from './coordinatorIdentity.js';
import { RegistryService } from './registryService.js';
import { FIXTURE_GAH_BINARY } from './fixtureGahHarness.js';

/** #1593 round 2: GitLab proxies Parasaur under a path prefix
 * (`X-Forwarded-Prefix: /parasaur`), so the browser-bound approval
 * credential must be scoped to `<prefix>/api/pairing/access` — otherwise
 * the prefixed status and claim requests lose their cookie and approval
 * sign-in fails. The cookie path reuses the same validated public prefix
 * the served documents' bootstrap uses. */
test('the device-approval cookie path follows the validated public prefix', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'gah-pairing-prefix-'));
  const keys = ['COORDINATOR_TOKEN', 'GAH_ALLOW_INSECURE_HTTP', 'GAH_COORDINATOR_IDENTITY_PATH', 'GAH_WS_AUTH_MODE', 'GAH_SKILL_BANK_PATH',
    'GAH_GATEWAY_SETTINGS_PATH', 'GAH_MANAGER_CHAT_SETTINGS_PATH', 'GAH_BINARY', 'GAH_FIXTURE_PROFILE_LIST'] as const;
  const saved = keys.map(key => process.env[key]);
  process.env.COORDINATOR_TOKEN = 'owner-fixture-secret';
  delete process.env.GAH_ALLOW_INSECURE_HTTP;
  delete process.env.GAH_WS_AUTH_MODE;
  process.env.GAH_COORDINATOR_IDENTITY_PATH = join(directory, 'identity.json');
  process.env.GAH_SKILL_BANK_PATH = join(directory, 'skills.json');
  process.env.GAH_GATEWAY_SETTINGS_PATH = join(directory, 'gateway.json');
  process.env.GAH_MANAGER_CHAT_SETTINGS_PATH = join(directory, 'chat.json');
  process.env.GAH_BINARY = FIXTURE_GAH_BINARY;
  process.env.GAH_FIXTURE_PROFILE_LIST = join(directory, 'profiles.json');
  writeFileSync(process.env.GAH_FIXTURE_PROFILE_LIST, '[]');
  writeFileSync(process.env.GAH_GATEWAY_SETTINGS_PATH, JSON.stringify({ url: 'https://gateway.example.test', apiKey: 'key', enabled: true }));
  writeFileSync(process.env.GAH_MANAGER_CHAT_SETTINGS_PATH, JSON.stringify({ defaultBackend: 'claude', profileOverrides: {} }));
  resetCachedCoordinatorIdentity();
  const access = new DeviceAccess(join(directory, 'devices.json'));
  const createRequest = access.createAccessRequest.bind(access);
  let requestCredential = '';
  access.createAccessRequest = (...args) => {
    const result = createRequest(...args);
    requestCredential = result.cookie;
    return result;
  };
  const server = http.createServer(createServer({ deviceAccess: access,
    registryService: new RegistryService(join(directory, 'registry.json')), detectTailscaleIPv4: async () => null }));
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  const origin = base.replace('http:', 'https:');
  // The documented install: nginx terminates TLS on loopback and forwards
  // the public scheme, so the direct request carries the proxy headers.
  const remote = { 'X-Forwarded-For': '198.51.100.2', 'X-Forwarded-Proto': 'https', Origin: origin };
  const owner = { ...remote, Authorization: 'Bearer owner-fixture-secret' };
  const post = (path: string, body: unknown, headers: Record<string, string> = remote) => fetch(base + '/api/pairing' + path,
    { method: 'POST', headers: { ...headers, 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  try {
    const handoff = await (await post('/handoff', {}, { ...remote, 'X-Forwarded-Prefix': '/parasaur' })).json() as { code: string; verifier: string };
    assert.equal((await post('/handoff/approve', { code: handoff.code }, owner)).status, 200);
    const session = await post('/handoff/claim', handoff, { ...remote, 'X-Forwarded-Prefix': '/parasaur' });
    assert.deepEqual(await session.json(), { token: 'owner-fixture-secret' });
    assert.equal((await post('/handoff/claim', handoff)).status, 410);
    // Without a prefix the cookie keeps the app-absolute path.
    const plain = await post('/access/request', { name: 'Root browser' });
    assert.equal(plain.status, 202);
    assert.match(plain.headers.getSetCookie()[0], /Path=\/api\/pairing\/access(?:;|$)/);
    // A validated prefix scopes the cookie to the prefixed request paths.
    const prefixed = await post('/access/request', { name: 'Prefixed browser' }, { ...remote, 'X-Forwarded-Prefix': '/parasaur' });
    assert.equal(prefixed.status, 202);
    const encryptedCookie = prefixed.headers.getSetCookie()[0];
    assert.ok(!encryptedCookie.includes(requestCredential));
    assert.ok(!encryptedCookie.includes(requestCredential.split('.')[1]));
    const status = await fetch(base + '/api/pairing/access/status', { headers: {
      ...remote, 'X-Forwarded-Prefix': '/parasaur', Cookie: encryptedCookie.split(';')[0] } });
    assert.equal(status.status, 200);
    const tampered = await fetch(base + '/api/pairing/access/status', { headers: {
      ...remote, Cookie: encryptedCookie.split(';')[0] + 'tampered' } });
    assert.equal(tampered.status, 404);
    assert.match(prefixed.headers.getSetCookie()[0], /Path=\/parasaur\/api\/pairing\/access(?:;|$)/);
    // A trailing slash in the header normalizes to the same path.
    const slashed = await post('/access/request', { name: 'Slashed browser' }, { ...remote, 'X-Forwarded-Prefix': '/parasaur/' });
    assert.match(slashed.headers.getSetCookie()[0], /Path=\/parasaur\/api\/pairing\/access(?:;|$)/);
    // An invalid prefix (origin smuggling, traversal, no leading slash)
    // changes nothing: the cookie falls back to the app-absolute path.
    for (const invalid of ['//evil.example', '/..', 'parasaur']) {
      const smuggled = await post('/access/request', { name: 'Smuggled browser' }, { ...remote, 'X-Forwarded-Prefix': invalid });
      assert.match(smuggled.headers.getSetCookie()[0], /Path=\/api\/pairing\/access(?:;|$)/, `rejects prefix ${invalid}`);
    }

    // The claim clears the request cookie with the same prefixed path it
    // was set with, so the browser-bound credential never outlives the claim.
    const request = await prefixed.json() as PairingAccessRequest;
    const approve = await post(`/access/requests/${request.id}/approve`, { matching_code: request.matching_code, confirm: true }, owner);
    assert.equal(approve.status, 200);
    const claim = await post('/access/claim', { confirm: true },
      { ...remote, 'X-Forwarded-Prefix': '/parasaur', Cookie: prefixed.headers.getSetCookie()[0].split(';')[0] });
    assert.equal(claim.status, 200);
    const cleared = claim.headers.getSetCookie().find(value => value.startsWith('gah_pairing_request='));
    assert.ok(cleared);
    assert.match(cleared, /Path=\/parasaur\/api\/pairing\/access(?:;|$)/);
  } finally {
    await new Promise<void>(resolve => server.close(() => resolve()));
    keys.forEach((key, index) => { if (saved[index] === undefined) delete process.env[key]; else process.env[key] = saved[index]; });
    resetCachedCoordinatorIdentity();
    rmSync(directory, { recursive: true, force: true });
  }
});
