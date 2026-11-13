import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { test } from 'node:test';
import express from 'express';
import rateLimit from 'express-rate-limit';
import { localChatAccounts } from './chatAccounts.js';
import { createWorkerChatRouter } from './workerChat.js';
import { parseWorkerChatReply } from './workerChatProtocol.js';
import { workerChatConnection } from './remoteChat.js';
import { RegistryService } from './registryService.js';
import { COORDINATOR_SCHEMA_DIGEST } from './coordinatorIdentity.js';
import { authMiddleware } from './authMiddleware.js';
import type { ProfileSummary } from '@git-agent-harness/contracts';

const account = { instance: 'work', backend: 'codex', label: 'Worker account', enabled: true, executableReady: true, authReady: null };
type ProfileConfig = Awaited<ReturnType<NonNullable<Parameters<typeof localChatAccounts>[2]>>>;

test('local chat account discovery projects only metadata for the requested runner', async () => {
  const read = async () => ({ backend_instances: [
    { backend_instance: 'work', logical_backend: 'codex', account_label: 'Worker account', enabled: true, executable_resolved: true,
      credential_id: 'PRIVATE_CREDENTIAL', state_root: '/private/path' },
    { backend_instance: 'other', logical_backend: 'claude', enabled: true }
  ] }) as unknown as ProfileConfig;
  const result = await localChatAccounts('project', 'codex', read);
  assert.deepEqual(result, [account]);
});

test('worker account replies reject wrong-runner, duplicate and malformed identities and discard private fields', () => {
  assert.deepEqual(parseWorkerChatReply({ accounts: [{ ...account, secret: 'PRIVATE', stateRoot: '/private' }] }, { action: 'accounts', backend: 'codex' }), { accounts: [account] });
  for (const accounts of [[{ ...account, backend: 'claude' }], [account, account], [{ ...account, instance: '../work' }], [{ ...account, authReady: 'yes' }], Array(129).fill(account)]) {
    assert.throws(() => parseWorkerChatReply({ accounts }, { action: 'accounts', backend: 'codex' }), /invalid chat response/);
  }
});

test('registered worker transport reads that node account list without starting an adapter and refuses identity mismatches', async t => {
  const root = mkdtempSync(join(tmpdir(), 'parasaur-node-accounts-'));
  const saved = process.env.COORDINATOR_TOKEN;
  process.env.COORDINATOR_TOKEN = 'synthetic-account-read-token';
  const profile = { ...JSON.parse(readFileSync(new URL('../tests/fixtures/gah/responses/profile-list.json', import.meta.url), 'utf8'))[0], name: 'project' } as ProfileSummary;
  const app = express();
  app.use(rateLimit({ windowMs: 60_000, limit: 120 }), express.json(), authMiddleware);
  let reads = 0;
  app.use('/api/worker-chat', createWorkerChatRouter({
    node: { role: 'worker', central_url: 'https://central.test' }, nodeId: 'worker', profiles: async () => [profile],
    accounts: async (name, backend) => { assert.equal(name, 'project'); assert.equal(backend, 'codex'); reads++; return [account]; },
    adapter: () => { throw new Error('Account discovery must not start a provider'); }
  }));
  const server = createServer(app);
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address(); assert.ok(address && typeof address !== 'string');
  const origin = `http://127.0.0.1:${address.port}`;
  const registry = new RegistryService(join(root, 'registry.json'));
  registry.registerNode({ node_id: 'worker', display_name: 'Worker', advertised_url: origin, version: '0.3.0', schema_digest: COORDINATOR_SCHEMA_DIGEST,
    transport_mode: 'loopback', secret_ref: 'env:COORDINATOR_TOKEN', profiles: ['project'] });
  t.after(async () => {
    server.closeAllConnections(); await new Promise<void>(resolve => server.close(() => resolve()));
    if (saved === undefined) delete process.env.COORDINATOR_TOKEN; else process.env.COORDINATOR_TOKEN = saved;
    rmSync(root, { recursive: true, force: true });
  });
  const connection = workerChatConnection(registry, 'worker', 'project', profile);
  assert.deepEqual(await connection.request({ action: 'accounts', backend: 'codex' }), { accounts: [account] });
  assert.equal(reads, 1);
  const wrong = await fetch(`${origin}/api/worker-chat`, { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action: 'accounts', backend: 'codex', profile: 'project', nodeId: 'other', repo: profile.repo, provider: profile.provider, origin: new URL(profile.web_url!).origin }) });
  assert.equal(wrong.status, 409);
  const mismatch = await fetch(`${origin}/api/worker-chat`, { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ action: 'accounts', backend: 'codex', profile: 'project', nodeId: 'worker', repo: 'another/repo', provider: profile.provider, origin: new URL(profile.web_url!).origin }) });
  assert.equal(mismatch.status, 409);
  const unauthenticated = await fetch(`${origin}/api/worker-chat`, { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Forwarded-For': '192.0.2.10' }, body: '{}' });
  assert.ok([401, 403].includes(unauthenticated.status));
  assert.equal(reads, 1);
});
