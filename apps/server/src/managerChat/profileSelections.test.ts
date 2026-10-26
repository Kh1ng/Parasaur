import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import type { ManagerAdapter } from './registry.js';
import { restoreProfileSelections } from './profileSelections.js';
import { modelOverrideForProfile, reasoningEffortOverrideForProfile, setModelOverrideForProfile, setReasoningEffortOverrideForProfile } from './settingsStore.js';

type Listing = Awaited<ReturnType<ManagerAdapter['listModels']>>;

function adapter(listing: Listing, available?: boolean) {
  const calls: string[] = [];
  const fake = {
    listModels: async () => listing,
    listModelsSnapshot: available === undefined ? undefined : async () => ({ version: 'v1', available, summary: listing }),
    setModel: async (_c: unknown, id: string) => { calls.push(`model:${id}`); },
    setReasoningEffort: async (_c: unknown, id: string) => { calls.push(`effort:${id}`); },
  };
  return { fake: fake as unknown as ManagerAdapter, calls };
}

const empty: Listing = { models: [], reasoningEfforts: [], currentModelId: undefined, currentReasoningEffortId: undefined } as unknown as Listing;

test('a best-effort empty listing never retires saved selections; a confirmed empty catalog does', async t => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-profile-selections-'));
  const previous = process.env.GAH_MANAGER_CHAT_SETTINGS_PATH;
  process.env.GAH_MANAGER_CHAT_SETTINGS_PATH = join(dir, 'settings.json');
  t.after(() => {
    if (previous === undefined) delete process.env.GAH_MANAGER_CHAT_SETTINGS_PATH; else process.env.GAH_MANAGER_CHAT_SETTINGS_PATH = previous;
    rmSync(dir, { recursive: true, force: true });
  });
  setModelOverrideForProfile('p', 'openhands', 'saved-model');
  setReasoningEffortOverrideForProfile('p', 'openhands', 'high');

  // OpenHands/ACP style: models: [] with no catalog confirmation at all.
  const unconfirmed = adapter(empty);
  await restoreProfileSelections('p', 'openhands', unconfirmed.fake, {});
  assert.equal(modelOverrideForProfile('p', 'openhands'), 'saved-model');
  assert.equal(reasoningEffortOverrideForProfile('p', 'openhands'), 'high');
  assert.deepEqual(unconfirmed.calls, []);

  // A catalog that reports itself unavailable is not proof either.
  const unavailable = adapter(empty, false);
  await restoreProfileSelections('p', 'openhands', unavailable.fake, {});
  assert.equal(modelOverrideForProfile('p', 'openhands'), 'saved-model');

  // Only a positively confirmed inventory retires the saved choice.
  const confirmed = adapter(empty, true);
  await restoreProfileSelections('p', 'openhands', confirmed.fake, {});
  assert.equal(modelOverrideForProfile('p', 'openhands'), undefined);
  assert.equal(reasoningEffortOverrideForProfile('p', 'openhands'), undefined);
});
