import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import express from 'express';
import type { ProfileSummary } from '@git-agent-harness/contracts';
import { createWorkerChatRouter } from './workerChat.js';
import { RegistryService } from './registryService.js';
import { getCoordinatorIdentity } from './coordinatorIdentity.js';
import { addRemoteProject } from './projectCatalog.js';
import { configureChatRouting } from './chatRouting.js';
import { setBackendForProfile } from './managerChat/settingsStore.js';
import { listModelsForProfile, setModelForProfile, setReasoningEffortForProfile } from './managerChat/ManagerChatManager.js';
import { createHeadlessBackend, agyBackendSpec } from './managerChat/headlessAdapter.js';
import { parseAgyModels } from './managerChat/agyModels.js';

test('remote AGY profile selects its worker model before applying per-model effort, including after worker restart', async t => {
  const root = mkdtempSync(join(tmpdir(), 'gah-remote-agy-'));
  const saved = { ...process.env };
  Object.assign(process.env, {
    GAH_BINARY: new URL('../tests/fixtures/gah/gah', import.meta.url).pathname,
    GAH_FIXTURE_PROFILE_LIST: join(root, 'profiles.json'),
    GAH_PROJECT_CATALOG_PATH: join(root, 'projects.json'),
    GAH_COORDINATOR_IDENTITY_PATH: join(root, 'identity.json'),
    GAH_MANAGER_CHAT_SETTINGS_PATH: join(root, 'manager.json'),
  });
  writeFileSync(process.env.GAH_FIXTURE_PROFILE_LIST!, '[]');
  const fixture: ProfileSummary = JSON.parse(readFileSync(new URL('../tests/fixtures/gah/responses/profile-list.json', import.meta.url), 'utf8'))[0];
  const profile = { ...fixture, name: 'worker-agy', local_path: root, worktree_base: join(root, 'worktrees') };
  const inventory = parseAgyModels(readFileSync(new URL('../../../tests/fixtures/agy/models.tsv', import.meta.url), 'utf8'), readFileSync(new URL('../../../tests/fixtures/agy/help.txt', import.meta.url), 'utf8'));
  const makeAdapter = () => createHeadlessBackend({ ...agyBackendSpec(), modelOptions: async () => inventory });
  let adapter = makeAdapter();
  const app = express();
  app.use(express.json());
  app.use('/api/worker-chat', createWorkerChatRouter({ node: { role: 'worker', central_url: 'https://central.test' }, nodeId: 'worker', profiles: async () => [profile], adapter: () => adapter }));
  const server = createServer(app);
  t.after(async () => {
    adapter.dispose?.();
    server.closeAllConnections();
    await new Promise<void>(resolve => server.close(() => resolve()));
    process.env = saved;
    rmSync(root, { recursive: true, force: true });
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const central = getCoordinatorIdentity();
  const registry = new RegistryService(join(root, 'registry.json'));
  registry.registerNode({ node_id: 'worker', display_name: 'Worker', advertised_url: `http://127.0.0.1:${address.port}`, version: central.version, schema_digest: central.schema_digest, transport_mode: 'loopback', secret_ref: 'env:COORDINATOR_TOKEN', profiles: [profile.name] });
  configureChatRouting(registry);
  const project = addRemoteProject('worker', profile);
  setBackendForProfile(project.chat_profile, 'agy');
  await setModelForProfile(project.chat_profile, 'gemini-3.8-flash-high', 'worker');
  assert.equal((await adapter.listModels({ key: profile.name, profile: profile.name })).currentModelId, 'gemini-3.8-flash-high');
  await setReasoningEffortForProfile(project.chat_profile, 'low', 'worker');
  assert.equal((await adapter.listModels({ key: profile.name, profile: profile.name })).currentReasoningEffortId, 'low');
  adapter.dispose?.();
  adapter = makeAdapter();
  await setReasoningEffortForProfile(project.chat_profile, 'high', 'worker');
  const selected = await listModelsForProfile(project.chat_profile, 'worker');
  assert.equal(selected.currentModelId, 'gemini-3.8-flash-high');
  assert.equal(selected.currentReasoningEffortId, 'high');
  await assert.rejects(setReasoningEffortForProfile(project.chat_profile, 'turbo', 'worker'));
});
