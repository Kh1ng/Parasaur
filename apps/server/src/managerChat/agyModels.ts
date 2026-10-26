/** AGY 1.3.6 exposes TSV models and the valid effort vocabulary in --help.
 * Cache each spec's installation for one minute; failed commands retain the last inventory. */
import { randomUUID } from 'node:crypto';
import { execFile as execFileCallback } from 'node:child_process';
import { promisify } from 'node:util';
import { agentChildEnv } from './childEnv.js';
import type { ManagerModelInfo, ManagerReasoningEffortInfo } from './registry.js';

const execFile = promisify(execFileCallback);
type Options = { models: ManagerModelInfo[]; reasoningEfforts: ManagerReasoningEffortInfo[];
  modelReasoningEfforts: Record<string, ManagerReasoningEffortInfo[]>; catalogVersion?: string; metadataAvailable?: boolean };

function modelRows(output: string): ManagerModelInfo[] {
  const models: ManagerModelInfo[] = [];
  for (const line of output.split('\n').map(line => line.trim()).filter(Boolean)) {
    if (line === 'Fetching available models...') continue;
    const columns = line.split('\t');
    if (columns.length !== 2 || !/^[a-zA-Z0-9_.-]+$/.test(columns[0]) || !columns[1]) throw new Error('Unknown AGY model metadata');
    models.push({ id: columns[0], name: columns[1] });
  }
  return models;
}

export function parseAgyModels(output: string, help: string): Options {
  const vocabulary = help.split('\n').find(line => /^\s*--effort\s/.test(line))?.match(/\(([^)]+)\)/)?.[1].split('|');
  if (!vocabulary?.length || vocabulary.some(effort => !/^[a-z]+$/.test(effort))) throw new Error('Unknown AGY effort metadata');
  const models = modelRows(output);
  const families = new Map<string, Set<string>>();
  const variants = new Map<string, string>();
  for (const model of models) {
    const effort = vocabulary.find(effort => model.id.endsWith(`-${effort}`)
      && model.name.toLowerCase().endsWith(`(${effort})`));
    if (!effort) continue;
    const family = model.id.slice(0, -effort.length - 1);
    variants.set(model.id, family);
    const efforts = families.get(family) ?? new Set<string>();
    efforts.add(effort);
    families.set(family, efforts);
  }
  const modelReasoningEfforts = Object.fromEntries(models.map(model => [model.id,
    vocabulary.filter(effort => families.get(variants.get(model.id) ?? model.id)?.has(effort))
      .map(id => ({ id, name: id[0].toUpperCase() + id.slice(1) }))]));
  return { models: models.map(model => ({ ...model, reasoningEfforts: modelReasoningEfforts[model.id] })), reasoningEfforts: [], modelReasoningEfforts };
}

export function createAgyModelOptions(runtime?: { executable: string; state_root: string | null }, deps: {
  now?: () => number;
  run?: (args: string[]) => Promise<{ stdout: string; stderr: string }>;
  warn?: (message: string) => void;
} = {}) {
  const now = deps.now ?? Date.now;
  const run = deps.run ?? (async (args: string[]) => {
    const result = execFile(runtime?.executable ?? 'agy', args, { encoding: 'utf8', timeout: 10_000,
      env: agentChildEnv(runtime?.state_root ? { HOME: runtime.state_root } : {}) });
    result.child.stdin?.end();
    return result;
  });
  let nextCheckAt = -Infinity;
  let cached: Options = { models: [], reasoningEfforts: [], modelReasoningEfforts: {} };
  let version = randomUUID();
  let metadataAvailable = false;
  let pending: Promise<Options> | undefined;
  let warning: string | undefined;
  let stopped = false;
  let timer: ReturnType<typeof setInterval> | undefined;
  function warn(message: string) {
    if (warning !== message) {
      warning = message;
      (deps.warn ?? console.warn)(message);
    }
  }
  function refresh(): Promise<Options> {
    if (stopped) return Promise.resolve(cached);
    if (pending) return pending;
    if (now() < nextCheckAt) return Promise.resolve(cached);
    pending = (async () => {
      const previous = { models: cached.models, reasoningEfforts: cached.reasoningEfforts,
        modelReasoningEfforts: cached.modelReasoningEfforts };
      try {
        const models = await run(['models']);
        const help = await run(['--help']);
        cached = parseAgyModels(models.stdout, help.stdout + help.stderr);
        metadataAvailable = true;
        warning = undefined;
      } catch {
        // Only parsed metadata can retire a saved selection.
        metadataAvailable = false;
        warn('[managerChat] AGY metadata unavailable; retaining the cached models and effort options.');
      }
      if (JSON.stringify(previous) !== JSON.stringify({ models: cached.models, reasoningEfforts: cached.reasoningEfforts,
        modelReasoningEfforts: cached.modelReasoningEfforts })) version = randomUUID();
      nextCheckAt = now() + (metadataAvailable ? 60_000 : 10_000);
      cached.catalogVersion = version;
      cached.metadataAvailable = metadataAvailable;
      return cached;
    })().finally(() => { pending = undefined; });
    return pending;
  }
  const read = (): Promise<Options> => {
    // Start with the backend's first inventory request, then keep its cache
    // fresh in the background like the CLIProxyAPI inventory refresher.
    if (!stopped && !timer) {
      timer = setInterval(() => { void refresh(); }, 10_000);
      timer.unref?.();
    }
    return refresh();
  };
  return Object.assign(read, { version: () => version, available: () => metadataAvailable,
    dispose: () => { stopped = true; clearInterval(timer); } });
}
