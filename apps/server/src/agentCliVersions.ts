import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import type { AgentCliVersion, AgentCliUpdate, NodeAgentVersions } from '@git-agent-harness/contracts';
import { findGahBinary } from './gahCli.js';
import { countActiveClaims } from './workerUpdate.js';
import { readAdminUpdateState } from './adminUpdate.js';
import { refreshUpdatedAgentModels } from './managerChat/registry.js';

import { AGENT_BACKENDS, readAgentUpdates } from './agentCliState.js';
export { readAgentUpdates } from './agentCliState.js';

const exec = promisify(execFile);
const UPDATABLE = new Set<string>(['claude', 'codex', 'opencode', 'vibe', 'agy']);
export const agentUpdatesEnabled = () => process.env.GAH_ENABLE_ADMIN_UPDATE === '1';
export const agentAutoUpdateEnabled = () => process.env.GAH_AUTO_UPDATE_AGENT_CLIS === '1';

export class AgentCliError extends Error {
  constructor(readonly status: number, readonly code: string, message: string) { super(message); }
}

interface Deps {
  probe?: () => Promise<AgentCliVersion[]>;
  launch?: (backend: string) => Promise<void>;
  readUpdates?: () => AgentCliUpdate[];
  activeClaims?: () => Promise<number>;
  enabled?: () => boolean;
  optedIn?: () => boolean;
  nodeUpdating?: () => boolean;
  refreshModels?: (backend: string) => void | Promise<void>;
  now?: () => number;
}

async function probeVersions(): Promise<AgentCliVersion[]> {
  const { stdout } = await exec(findGahBinary(), ['agents', 'versions', '--json'], { timeout: 25_000, maxBuffer: 256 * 1024 });
  const rows: unknown = JSON.parse(stdout);
  if (!Array.isArray(rows) || rows.length !== AGENT_BACKENDS.length
    || !AGENT_BACKENDS.every(b => rows.filter(r => r?.backend === b).length === 1)
    || rows.some(r => typeof r.outdated !== 'boolean'
      || ['installed', 'latest', 'source', 'update_command', 'error'].some(k => r[k] !== null && typeof r[k] !== 'string'))) {
    throw new Error('Invalid agent inventory');
  }
  return rows as AgentCliVersion[];
}

/** Detached Rust child owns the timeout, update lock and durable outcome. */
function launchUpdate(backend: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const child = spawn(findGahBinary(), ['agents', 'update', backend, '--yes'], { detached: true, stdio: 'ignore' });
    child.once('error', reject);
    child.once('close', code => code === 0 ? resolve() : reject(new Error('Agent update failed')));
    child.unref();
  });
}

/** One service per node: single-flight probes and serialized owner/scheduled updates. */
export class AgentCliVersions {
  private cached: { rows: AgentCliVersion[]; at: number } | undefined;
  private probePending: Promise<AgentCliVersion[]> | undefined;
  private updating = false;
  private generation = 0;
  private launchError: string | null = null;
  private readonly observed = new Map<string, string>();

  constructor(readonly node: { node_id: string; display_name: string }, private readonly deps: Deps = {}) {}

  private updates(): AgentCliUpdate[] {
    const updates = (this.deps.readUpdates ?? readAgentUpdates)();
    for (const update of updates) {
      const revision = `${update.started_at}:${update.status}`;
      if (this.observed.get(update.backend) === revision) continue;
      this.observed.set(update.backend, revision);
      this.cached = undefined;
      this.generation++;
      if (update.status === 'success') {
        void Promise.resolve(this.deps.refreshModels
          ? this.deps.refreshModels(update.backend)
          : refreshUpdatedAgentModels()).catch(() => undefined);
      }
    }
    return updates;
  }

  private versions(): Promise<AgentCliVersion[]> {
    const now = this.deps.now ?? Date.now;
    if (this.cached && now() - this.cached.at < 5 * 60_000) return Promise.resolve(this.cached.rows);
    if (this.probePending) return this.probePending;
    const generation = this.generation;
    this.probePending = (this.deps.probe ?? probeVersions)().then(rows => {
      if (generation === this.generation) this.cached = { rows, at: now() };
      return rows;
    }).finally(() => { this.probePending = undefined; });
    return this.probePending;
  }

  async snapshot(): Promise<NodeAgentVersions> {
    const updates = this.updates();
    return { ...this.node, versions: await this.versions(), updates,
      auto_update: (this.deps.optedIn ?? agentAutoUpdateEnabled)(),
      updates_enabled: (this.deps.enabled ?? agentUpdatesEnabled)(), error: this.launchError };
  }

  async start(backend: string): Promise<void> {
    if (!UPDATABLE.has(backend)) throw new AgentCliError(400, 'unsupported_agent', 'Choose a supported agent CLI.');
    if (!(this.deps.enabled ?? agentUpdatesEnabled)()) throw new AgentCliError(403, 'agent_updates_disabled', 'Agent updates are disabled on this node.');
    if (this.updating || this.updates().some(u => u.status === 'running')
      || (this.deps.nodeUpdating ?? (() => readAdminUpdateState().status === 'running'))()) {
      throw new AgentCliError(409, 'agent_update_busy', 'An update is already running on this node.');
    }
    this.updating = true;
    try {
      // Fail closed on unreadable claims; do not modify CLIs during dispatch.
      if (await (this.deps.activeClaims ?? countActiveClaims)() !== 0) throw new AgentCliError(409, 'node_busy', 'Wait for active dispatches to finish.');
      const rows = await this.versions();
      if (!rows.find(r => r.backend === backend)?.installed) throw new AgentCliError(409, 'agent_not_installed', 'This CLI is not installed on the node.');
      this.launchError = null;
      const work = (this.deps.launch ?? launchUpdate)(backend);
      void work.catch(() => { this.launchError = `${backend} update failed. Check the node update state and installation permissions.`; }).finally(() => {
        this.updating = false;
        this.cached = undefined;
        this.generation++;
        this.updates();
      }).catch(() => { this.launchError = 'Cannot read the final agent update state.'; });
    } catch (error) {
      this.updating = false;
      throw error;
    }
  }

  /** Same timer as node releases, with a separate local opt-in, default off. */
  async scheduledUpdate(): Promise<void> {
    const updates = this.updates();
    if (!(this.deps.optedIn ?? agentAutoUpdateEnabled)() || !(this.deps.enabled ?? agentUpdatesEnabled)()) return;
    const rows = await this.versions();
    // One CLI per tick avoids a burst of installers and lets claims drain again.
    const attempted = (backend: string) => updates.find(u => u.backend === backend)?.started_at ?? '';
    const candidate = rows.filter(r => r.outdated && r.installed && UPDATABLE.has(r.backend))
      .sort((a, b) => attempted(a.backend).localeCompare(attempted(b.backend)))[0];
    if (candidate) await this.start(candidate.backend);
  }
}
