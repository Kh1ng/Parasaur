import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'node:http';
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import express from 'express';
import type { ProfileSummary } from '@git-agent-harness/contracts';
import { createWorkerChatRouter } from './workerChat.js';
import { RegistryService } from './registryService.js';
import { getCoordinatorIdentity } from './coordinatorIdentity.js';
import { addRemoteProject } from './projectCatalog.js';
import { configureChatRouting } from './chatRouting.js';
import { modelOverrideForProfile, reasoningEffortOverrideForProfile, setBackendForProfile, setModelOverrideForProfile } from './managerChat/settingsStore.js';
import { listModelsForProfile, sendManagerChatMessage, setSessionLogOptions, setModelForProfile, setReasoningEffortForProfile } from './managerChat/ManagerChatManager.js';
import { createHeadlessBackend, agyBackendSpec } from './managerChat/headlessAdapter.js';
import { resolveAdapter } from './managerChat/registry.js';
import { parseAgyModels } from './managerChat/agyModels.js';

for (const remote of [true, false]) test(`${remote ? 'remote' : 'local'} AGY persisted selections are validated before the next CLI turn`, async t => {
  const root = mkdtempSync(join(tmpdir(), 'gah-remote-agy-'));
  const saved = { ...process.env };
  Object.assign(process.env, {
    GAH_GATEWAY_SETTINGS_PATH: join(root, 'gateway.json'),
    TDAI_GATEWAY_URL: 'http://127.0.0.1:1',
    GAH_BINARY: new URL('../tests/fixtures/gah/gah', import.meta.url).pathname,
    GAH_FIXTURE_PROFILE_LIST: join(root, 'profiles.json'),
    GAH_PROJECT_CATALOG_PATH: join(root, 'projects.json'),
    GAH_COORDINATOR_IDENTITY_PATH: join(root, 'identity.json'),
    GAH_MANAGER_CHAT_SETTINGS_PATH: join(root, 'manager.json'),
  });
  writeFileSync(process.env.GAH_FIXTURE_PROFILE_LIST!, '[]');
  const fixture: ProfileSummary = JSON.parse(readFileSync(new URL('../tests/fixtures/gah/responses/profile-list.json', import.meta.url), 'utf8'))[0];
  const profile = { ...fixture, name: 'worker-agy', local_path: root, worktree_base: join(root, 'worktrees') };
  let inventory = parseAgyModels(readFileSync(new URL('../../../tests/fixtures/agy/models.tsv', import.meta.url), 'utf8'), readFileSync(new URL('../../../tests/fixtures/agy/help.txt', import.meta.url), 'utf8'));
  const argv = join(root, 'argv.json');
  const executable = join(root, 'agy');
  writeFileSync(executable, `#!/usr/bin/env node
require('fs').writeFileSync(${JSON.stringify(argv)}, JSON.stringify(process.argv.slice(2)));
console.log(JSON.stringify({event:'result', result:{status:'SUCCESS', response:'ok'}}));
`);
  chmodSync(executable, 0o755);
  let modelReads = 0;
  let modelWrites = 0;
  let effortWrites = 0;
  const makeAdapter = () => {
    const backend = createHeadlessBackend({ ...agyBackendSpec({ executable, state_root: null }), modelOptions: async () => inventory });
    return { ...backend,
      listModels: async (...args: Parameters<typeof backend.listModels>) => { modelReads++; return backend.listModels(...args); },
      setModel: async (...args: Parameters<typeof backend.setModel>) => { modelWrites++; return backend.setModel(...args); },
      setReasoningEffort: async (...args: Parameters<typeof backend.setReasoningEffort>) => { effortWrites++; return backend.setReasoningEffort(...args); }
    };
  };
  const localAdapter = resolveAdapter('agy');
  const originalAdapter = { ...localAdapter };
  setSessionLogOptions({ stateDir: root });
  const warnings: string[] = [];
  t.mock.method(console, 'warn', (message: string) => warnings.push(message));
  let adapter = makeAdapter();
  const app = express();
  app.use(express.json());
  app.get('/api/status', (_req, res) => res.json({ node_id: 'worker', generated_at: new Date().toISOString(), profile: { profile: profile.name }, backend_configured: { agy: true }, backend_instances: [], availability: [] }));
  app.use('/api/worker-chat', createWorkerChatRouter({ node: { role: 'worker', central_url: 'https://central.test' }, nodeId: 'worker', profiles: async () => [profile], adapter: () => adapter }));
  const server = createServer(app);
  t.after(async () => {
    adapter.dispose?.();
    Object.assign(localAdapter, originalAdapter);
    setSessionLogOptions({});
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
  await registry.getNodeObservations();
  configureChatRouting(registry);
  const project = addRemoteProject('worker', profile);
  const chatProfile = remote ? project.chat_profile : profile.name;
  const nodeId = remote ? 'worker' : undefined;
  if (!remote) {
    writeFileSync(process.env.GAH_FIXTURE_PROFILE_LIST!, JSON.stringify([profile]));
    Object.assign(localAdapter, adapter);
  }
  setBackendForProfile(chatProfile, 'agy');
  await setModelForProfile(chatProfile, 'gemini-3.8-flash-high', nodeId);
  assert.equal((await adapter.listModels({ key: profile.name, profile: profile.name })).currentModelId, 'gemini-3.8-flash-high');
  await setReasoningEffortForProfile(chatProfile, 'low', nodeId);
  assert.equal((await adapter.listModels({ key: profile.name, profile: profile.name })).currentReasoningEffortId, 'low');
  const beforeTurns = [modelReads, modelWrites, effortWrites];
  for (let turn = 0; turn < 3; turn++) {
    await sendManagerChatMessage(chatProfile, '/status', undefined, undefined, undefined, nodeId);
    assert.deepEqual([modelReads - beforeTurns[0], modelWrites - beforeTurns[1], effortWrites - beforeTurns[2]], [1, 0, 0],
      'three unchanged turns restore once and add no further worker model/effort calls');
  }
  adapter.dispose?.();
  adapter = makeAdapter();
  if (!remote) Object.assign(localAdapter, adapter);
  await listModelsForProfile(chatProfile, nodeId); // Catalog refresh restores a recreated adapter.
  await setReasoningEffortForProfile(chatProfile, 'high', nodeId);
  const selected = await listModelsForProfile(chatProfile, nodeId);
  assert.equal(selected.currentModelId, 'gemini-3.8-flash-high');
  assert.equal(selected.currentReasoningEffortId, 'high');
  setModelOverrideForProfile(chatProfile, 'agy', 'retired-saved-model');
  const beforeEffortChange = [modelReads, modelWrites];
  await setReasoningEffortForProfile(chatProfile, 'low', nodeId);
  assert.equal(modelOverrideForProfile(chatProfile, 'agy'), 'retired-saved-model');
  assert.equal(reasoningEffortOverrideForProfile(chatProfile, 'agy'), 'low');
  await assert.rejects(setReasoningEffortForProfile(chatProfile, 'turbo', nodeId));
  assert.equal(modelOverrideForProfile(chatProfile, 'agy'), 'retired-saved-model');
  assert.equal(reasoningEffortOverrideForProfile(chatProfile, 'agy'), 'low');
  assert.deepEqual([modelReads, modelWrites], beforeEffortChange, 'effort changes only validate effort on the current model');
  setModelOverrideForProfile(chatProfile, 'agy', 'gemini-3.8-flash-high');
  await sendManagerChatMessage(chatProfile, '/status', undefined, undefined, undefined, nodeId);
  assert.ok(JSON.parse(readFileSync(argv, 'utf8')).includes('--effort'));
  await setModelForProfile(chatProfile, 'claude-sonnet-4-6', nodeId);
  await sendManagerChatMessage(chatProfile, '/status', undefined, undefined, undefined, nodeId);
  const switched: string[] = JSON.parse(readFileSync(argv, 'utf8'));
  assert.ok(switched.includes('claude-sonnet-4-6'));
  assert.ok(!switched.includes('--effort'), 'unsupported persisted effort must not reach the CLI');
  assert.equal(reasoningEffortOverrideForProfile(chatProfile, 'agy'), undefined);
  assert.equal(warnings.filter(w => w.includes('persisted')).length, 1);

  // A saved selection can outlive both the adapter and its provider catalog.
  setModelOverrideForProfile(chatProfile, 'agy', 'retired-model');
  await sendManagerChatMessage(chatProfile, '/status', undefined, undefined, undefined, nodeId);
  assert.ok(!JSON.parse(readFileSync(argv, 'utf8')).includes('--model'));
  assert.equal(modelOverrideForProfile(chatProfile, 'agy'), undefined);
  await listModelsForProfile(chatProfile, nodeId);
  assert.equal(warnings.filter(w => w.includes('persisted')).length, 2);

  await setModelForProfile(chatProfile, 'gemini-3.8-flash-high', nodeId);
  await setReasoningEffortForProfile(chatProfile, 'high', nodeId);
  inventory = { models: [], reasoningEfforts: [], modelReasoningEfforts: {} };
  await listModelsForProfile(chatProfile, nodeId);
  await listModelsForProfile(chatProfile, nodeId);
  assert.equal(modelOverrideForProfile(chatProfile, 'agy'), undefined);
  assert.equal(reasoningEffortOverrideForProfile(chatProfile, 'agy'), undefined);
  assert.equal(warnings.filter(w => w.includes('persisted')).length, 3);
  await assert.rejects(setReasoningEffortForProfile(chatProfile, 'turbo', nodeId));
});
