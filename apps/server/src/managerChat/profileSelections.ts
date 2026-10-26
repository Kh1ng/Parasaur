import type { ManagerAdapter } from './registry.js';
import { modelOverrideForProfile, reasoningEffortOverrideForProfile, setModelOverrideForProfile, setReasoningEffortOverrideForProfile } from './settingsStore.js';

type Summary = Awaited<ReturnType<ManagerAdapter['listModels']>>;
const restored = new WeakMap<object, Map<string, { selection: string; version: string | undefined; summary: Summary }>>();
const selection = (profile: string, backend: string) => JSON.stringify([
  modelOverrideForProfile(profile, backend), reasoningEffortOverrideForProfile(profile, backend)
]);

/** Turns reuse validation until selections, inventory, or registration change.
 * Remote turns check only the worker catalog revision on a cache hit. */
export async function restoreProfileSelections(profile: string, backendId: string, adapter: ManagerAdapter,
  scope: object = adapter.listModels, refresh = true) {
  let cache = restored.get(scope);
  if (!cache) { cache = new Map(); restored.set(scope, cache); }
  const key = JSON.stringify([profile, backendId]);
  const previous = cache.get(key);
  const version = await adapter.modelCatalogVersion?.();
  if (!refresh && previous?.selection === selection(profile, backendId) && previous.version === version) return previous.summary;
  const summary = await validateSelections(profile, backendId, adapter);
  cache.set(key, { selection: selection(profile, backendId), version: await adapter.modelCatalogVersion?.(), summary });
  return summary;
}

async function validateSelections(profile: string, backendId: string, adapter: ManagerAdapter) {
  const conversation = { key: profile, profile };
  let summary = await adapter.listModels(conversation);
  const model = modelOverrideForProfile(profile, backendId);
  const effort = reasoningEffortOverrideForProfile(profile, backendId);
  if (model && !summary.models.some(item => item.id === model)) {
    setModelOverrideForProfile(profile, backendId, null);
    setReasoningEffortOverrideForProfile(profile, backendId, null);
    console.warn(`[managerChat] Cleared unsupported persisted model and effort for ${profile}/${backendId}; using the backend default.`);
    return summary;
  }
  if (model && model !== summary.currentModelId) {
    await adapter.setModel(conversation, model);
    summary = await adapter.listModels(conversation);
  }
  const efforts = summary.models.find(item => item.id === (model ?? summary.currentModelId))?.reasoningEfforts ?? summary.reasoningEfforts;
  if (effort && !efforts.some(item => item.id === effort)) {
    setReasoningEffortOverrideForProfile(profile, backendId, null);
    console.warn(`[managerChat] Cleared unsupported persisted effort for ${profile}/${backendId}.`);
  } else if (effort && effort !== summary.currentReasoningEffortId) {
    await adapter.setReasoningEffort(conversation, effort);
    summary = await adapter.listModels(conversation);
  }
  return summary;
}

