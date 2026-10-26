/**
 * Manager backend registry for the manager-chat MVP.
 *
 * Hermes and OpenCode speak ACP natively; Codex and Claude use their
 * official ACP bridge packages. Vibe, AGY, and OpenHands use the headless adapter,
 * which preserves transcript replay; AGY also advertises its CLI-native
 * model and reasoning-effort options, Vibe its config-native models (#1510),
 * and OpenHands its profile provider's models with the configured one
 * marked (#1512).
 */

import {
  createAcpBackend,
  hermesSpawnSpec,
  codexSpawnSpec,
  claudeSpawnSpec,
  opencodeSpawnSpec,
  type ManagerCommandInfo,
  type ManagerModelInfo,
  type ManagerReasoningEffortInfo,
  type SpawnSpec
} from './acpAdapter.js';
import { createHeadlessBackend, vibeBackendSpec, agyBackendSpec, openhandsBackendSpec } from './headlessAdapter.js';
import type { ChatTranscriptTurn, ChatUsage } from '@git-agent-harness/contracts';
import type { PermissionAsk } from './permissionPolicy.js';
import { runBackendInstanceRuntime } from '../gahCli.js';
import { bindOpenCodeModels, instanceAcpSpawn, instanceHeadlessSpec } from './instanceLaunch.js';
import { findInstalledExecutable } from './installedExecutable.js';
import { readAgentUpdates } from '../agentCliState.js';
import { createCliRouterModelRefresher } from '../cliRouter.js';

export type { ManagerCommandInfo, ManagerModelInfo, ManagerReasoningEffortInfo };

export interface ManagerBackendInfo {
  id: string;
  displayName: string;
  implemented: boolean;
}

/** Conversation identity and its real GAH profile. Keys are opaque and must
 * never be parsed to resolve profile-scoped configuration. */
export interface ManagerConversation {
  key: string;
  profile: string;
}

export interface ManagerAdapter extends ManagerBackendInfo {
  runTurn(
    conversation: ManagerConversation,
    input: {
      prompt: string;
      history: ChatTranscriptTurn[];
      onChunk: (text: string) => void;
      onToolResult: (name: string, text: string) => void;
      /** Session working directory (WP2); omitted = the server's cwd. */
      cwd?: string;
      /** Model override for this conversation (WP2 sessions). */
      model?: string | null;
      /** Per-session reasoning effort (WP2 sessions); backends without the
       * control ignore it rather than failing the turn. */
      reasoningEffort?: string | null;
      /** Structured tool-call stream (slice 3). */
      onToolCall?: (tool: {
        toolCallId: string;
        name: string | null;
        title: string;
        kind: string | null;
        status: 'pending' | 'completed' | 'failed';
        locations: string[];
        summary: string | null;
      }) => void;
      /** Permission round-trip (slice 3). */
      requestPermission?: (request: PermissionAsk) => Promise<string>;
    }
  ): Promise<{ reply: string; model: string | null; usage: ChatUsage | null }>;
  listCommands(gahProfile: string): Promise<ManagerCommandInfo[]>;
  listModels(conversation: ManagerConversation, cwd?: string): Promise<{
    models: ManagerModelInfo[];
    currentModelId: string | null;
    reasoningEfforts: ManagerReasoningEffortInfo[];
    currentReasoningEffortId: string | null;
    /** Context-window occupancy (issue #865); null for backends that don't
     * advertise it (only Hermes does today, via ACP's usage_update). */
    contextUsage: { size: number; used: number } | null;
    /** True when the backend runs its bundled SDK binary because no CLI is
     * installed, so its model list may differ from the CLI's. Absent means
     * false (older workers and backends without a bundled fallback). */
    bundledRuntime?: boolean;
  }>;
  setModel(conversation: ManagerConversation, modelId: string): Promise<void>;
  setReasoningEffort(conversation: ManagerConversation, effortId: string): Promise<void>;
  steerTurn(gahProfile: string, message: string): Promise<{ outcome: 'injected' }>;
  cancelTurn(gahProfile: string): Promise<void>;
  /** Release processes when a selected credential or instance changes. */
  dispose?(): void;
}

function acpManagerAdapter(
  id: string,
  displayName: string,
  spawnSpec: () => import('./acpAdapter.js').SpawnSpec,
  options?: { consecutiveFailureReconnectThreshold?: number }
): ManagerAdapter {
  const backend = createAcpBackend(displayName, spawnSpec, options);
  return { id, displayName, implemented: true, ...backend };
}

/** Default Claude Manager Chat runs the `claude` that dispatch resolves, so
 * its model list is that CLI's own (#1507). With none installed, the ACP
 * bridge falls back to the SDK binary pinned in package-lock.json.
 * Each new connection resolves again, so installing or removing `claude`
 * takes effect on the next spawn, and the connection keeps the provenance of
 * the spawn that started it. */
export function defaultClaudeSpawn(searchPath?: string): SpawnSpec & { bundledRuntime: boolean } {
  const executable = findInstalledExecutable('claude', searchPath);
  return {
    ...claudeSpawnSpec(executable ? { executable, state_root: null } : undefined),
    bundledRuntime: executable === null
  };
}

const DEFAULT_ADAPTERS: Record<string, () => ManagerAdapter> = {
  hermes: () => acpManagerAdapter('hermes', 'Hermes', hermesSpawnSpec),
  codex: () => acpManagerAdapter('codex', 'Codex', codexSpawnSpec, { consecutiveFailureReconnectThreshold: 2 }),
  claude: () => acpManagerAdapter('claude', 'Claude', () => defaultClaudeSpawn()),
  opencode: () => acpManagerAdapter('opencode', 'OpenCode', opencodeSpawnSpec),
  vibe: () => ({ ...createHeadlessBackend(vibeBackendSpec()) }) as ManagerAdapter,
  agy: () => ({ ...createHeadlessBackend(agyBackendSpec()) }) as ManagerAdapter,
  openhands: () => ({ ...createHeadlessBackend(openhandsBackendSpec()) }) as ManagerAdapter
};

const REGISTRY = Object.fromEntries(Object.entries(DEFAULT_ADAPTERS).map(([id, create]) => [id, create()]));

export const DEFAULT_BACKEND_ID = 'hermes';

export function listManagerBackends(): ManagerBackendInfo[] {
  return Object.values(REGISTRY).map(({ id, displayName, implemented }) => ({ id, displayName, implemented }));
}

export function resolveAdapter(backendId: string): ManagerAdapter {
  void refreshUpdatedAgentModels();
  const adapter = REGISTRY[backendId];
  if (!adapter) {
    throw new Error(`Unknown manager backend "${backendId}"`);
  }
  return adapter;
}

const INSTANCE_ADAPTERS = new Map<string, { runtime: string; runnerKind: string; adapter: ManagerAdapter }>();

/** Resolve one named account to its own ACP process and connection map. */
export async function resolveInstanceAdapter(profile: string, backendId: string, instance?: string | null): Promise<ManagerAdapter> {
  if (!instance) return resolveAdapter(backendId);
  void refreshUpdatedAgentModels();
  const key = `${profile}\0${instance}`;
  const runtime = await runBackendInstanceRuntime(profile, instance);
  if (runtime.logical_backend !== backendId) throw new Error(`Backend instance "${instance}" does not serve ${backendId}.`);
  const runtimeKey = JSON.stringify(runtime);
  const cached = INSTANCE_ADAPTERS.get(key);
  if (cached?.runtime === runtimeKey) return cached.adapter;
  const label = runtime.account_label ? `${backendId} · ${runtime.account_label}` : `${backendId} · ${instance}`;
  let adapter: ManagerAdapter;
  switch (runtime.runner_kind) {
    case 'codex':
      adapter = acpManagerAdapter(backendId, label, () => instanceAcpSpawn(profile, runtime, codexSpawnSpec(runtime)), { consecutiveFailureReconnectThreshold: 2 });
      break;
    case 'claude':
      adapter = acpManagerAdapter(backendId, label, () => instanceAcpSpawn(profile, runtime, claudeSpawnSpec(runtime)));
      break;
    case 'opencode':
      adapter = acpManagerAdapter(backendId, label, () => instanceAcpSpawn(profile, runtime, opencodeSpawnSpec(runtime)));
      break;
    case 'hermes':
      adapter = acpManagerAdapter(backendId, label, () => instanceAcpSpawn(profile, runtime, hermesSpawnSpec()));
      break;
    case 'vibe':
    case 'agy':
    case 'openhands': {
      const spec = runtime.runner_kind === 'vibe' ? vibeBackendSpec({ executable: runtime.executable, instance, stateRoot: runtime.state_root })
        : runtime.runner_kind === 'agy' ? agyBackendSpec() : openhandsBackendSpec({ instanceCredential: Boolean(runtime.credential_id) });
      adapter = { ...createHeadlessBackend(instanceHeadlessSpec(profile, runtime, spec)), displayName: label } as ManagerAdapter;
      break;
    }
    default: throw new Error(`Backend instance "${instance}" is not supported by Manager Chat.`);
  }
  adapter = bindOpenCodeModels(adapter, runtime);
  cached?.adapter.dispose?.();
  INSTANCE_ADAPTERS.set(key, { runtime: runtimeKey, runnerKind: runtime.runner_kind, adapter });
  return adapter;
}

/** New connections discover models from the updated CLI; active prompts finish. */
export function invalidateAgentModels(backend: string): void {
  REGISTRY[backend]?.dispose?.();
  if (DEFAULT_ADAPTERS[backend]) REGISTRY[backend] = DEFAULT_ADAPTERS[backend]();
  for (const [key, entry] of INSTANCE_ADAPTERS) {
    if (entry.runnerKind !== backend) continue;
    entry.adapter.dispose?.();
    INSTANCE_ADAPTERS.delete(key);
  }
}

const modelUpdateRevisions = new Map<string, string>();
const updatedRouterModels = createCliRouterModelRefresher();

/** Also observes terminal updates made outside this server, before reusing models. */
export function refreshUpdatedAgentModels(): Promise<void> {
  let updates;
  try { updates = readAgentUpdates(); } catch { return Promise.resolve(); }
  let changed = false;
  for (const update of updates) {
    if (update.status !== 'success' || modelUpdateRevisions.get(update.backend) === update.finished_at) continue;
    modelUpdateRevisions.set(update.backend, update.finished_at ?? update.started_at);
    invalidateAgentModels(update.backend);
    changed = true;
  }
  return changed ? updatedRouterModels.refresh() : Promise.resolve();
}
