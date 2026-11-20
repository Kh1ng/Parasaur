/**
 * Worker-side release update service (issue #1416). Central asks a worker
 * to update; the worker pulls the same release artifact central installs
 * (`gah update --from-release --role worker`) instead of rebuilding from
 * source. A worker that is mid-dispatch finishes its run first: `start`
 * arms the update and a drain poll launches it once the last active claim
 * clears, so a dispatch is never killed by its own node's update.
 *
 * State lives on disk like the central admin update (adminUpdate.ts): the
 * update may end by restarting this very node server (macOS LaunchAgent),
 * which would wipe any in-memory tracking before central could observe it.
 */
import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { basename, dirname, resolve } from 'node:path';
import { homedir } from 'node:os';
import type { WorkerUpdateStatus } from '@git-agent-harness/contracts';
import { COORDINATOR_VERSION } from '@git-agent-harness/contracts';
import { countActiveDispatches, findGahBinary } from './gahCli.js';
import { persistReleaseManifest, selectReleaseManifest } from './releaseFeed.js';
import { buildCommit, runningBuild } from './buildRevision.js';

const MAX_OUTPUT_CHARS = 200_000;
const DRAIN_POLL_MS = 15_000;

/** systemctl show works on Ubuntu 22.04 and later. Only a matching main
 * process in this exact service invocation may request its own restart. */
export function workerServiceUnit(properties: string, pid: number, invocation: string | undefined): string {
  if (invocation && /^[a-f0-9]{32}$/.test(invocation)) {
    for (const block of properties.trim().split(/\n\s*\n/)) {
      const lines = block.split('\n');
      const unit = /^Id=(gah-worker(?:@[\w.-]+)?\.service)$/m.exec(block)?.[1];
      if (unit && lines.includes(`MainPID=${pid}`) && lines.includes(`InvocationID=${invocation}`)) return unit;
    }
  }
  throw Error('Worker is not the main process of its systemd user service');
}

interface WorkerUpdateRecord extends WorkerUpdateStatus {
  pid: number | null;
  selectedManifest?: string;
  restart?: { previousInvocation:string; bundle:string; requestedAt:number };
}

/** Outside the Git checkout, same reasoning as adminUpdate.ts. */
export function workerUpdateStatePath(): string {
  const override = process.env.PARASAUR_WORKER_UPDATE_STATE_PATH;
  return override ? resolve(override) :
    resolve(process.env.XDG_STATE_HOME || resolve(homedir(), '.local', 'state'), 'gah', 'worker-update-state.json');
}

function idleStatus(): WorkerUpdateStatus {
  return {
    status: 'idle',
    current_version: COORDINATOR_VERSION,
    current_revision: runningBuild.revision,
    target_version: null,
    armed_at: null,
    started_at: null,
    finished_at: null,
    active_dispatches: null,
    output: ''
  };
}

function pidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function writeState(record: WorkerUpdateRecord): void {
  const path = workerUpdateStatePath();
  const dir = dirname(path);
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
  const tmpPath = `${path}.${process.pid}.tmp`;
  writeFileSync(tmpPath, JSON.stringify(record, null, 2), { mode: 0o600 });
  renameSync(tmpPath, path);
}

function view(record: WorkerUpdateRecord): WorkerUpdateStatus {
  const { pid: _pid, restart: _restart, selectedManifest: _manifest, ...status } = record;
  return status;
}

export interface WorkerUpdateDeps {
  spawnFn?: typeof spawn;
  /** Override for `gah claims active-count`; tests inject a stub. */
  countActiveClaims?: () => Promise<number>;
  /** Override for the post-success worker restart (macOS LaunchAgent);
   * tests inject a stub so nothing real is spawned. */
  restartWorker?: () => void;
  cwd?: string;
}

export class WorkerUpdateService {
  private drainTimer: NodeJS.Timeout | null = null;
  private updaterPid?: number;
  private starting?: Promise<{ started: boolean; status: WorkerUpdateStatus }>;
  private readonly spawnFn: typeof spawn;
  private readonly cwd: string;

  constructor(private readonly deps: WorkerUpdateDeps = {}) {
    this.spawnFn = deps.spawnFn ?? spawn;
    this.cwd = deps.cwd ?? process.cwd();
    if (this.read().status === 'waiting') this.scheduleDrain();
  }

  private read(): WorkerUpdateRecord {
    const path = workerUpdateStatePath();
    if (!existsSync(path)) return { ...idleStatus(), pid: null };
    try {
      const parsed = JSON.parse(readFileSync(path, 'utf8')) as Partial<WorkerUpdateRecord>;
      const base = idleStatus();
      const record: WorkerUpdateRecord = {
        status:
          parsed.status === 'waiting' || parsed.status === 'running' || parsed.status === 'success'
          || parsed.status === 'failed' || parsed.status === 'inferred_restart'
            ? parsed.status
            : 'idle',
        current_version: base.current_version,
        current_revision: base.current_revision,
        target_revision: buildCommit(parsed.target_revision),
        selectedManifest: parsed.selectedManifest,
        target_version: typeof parsed.target_version === 'string' ? parsed.target_version : null,
        armed_at: typeof parsed.armed_at === 'string' ? parsed.armed_at : null,
        started_at: typeof parsed.started_at === 'string' ? parsed.started_at : null,
        finished_at: typeof parsed.finished_at === 'string' ? parsed.finished_at : null,
        active_dispatches:
          typeof parsed.active_dispatches === 'number' && Number.isFinite(parsed.active_dispatches)
            ? parsed.active_dispatches
            : null,
        output: typeof parsed.output === 'string' ? parsed.output : '',
        pid: typeof parsed.pid === 'number' ? parsed.pid : null,
        restart: parsed.restart
      };
      if (record.status === 'inferred_restart' && record.restart) {
        const newInvocation = !!process.env.INVOCATION_ID && process.env.INVOCATION_ID !== record.restart.previousInvocation;
        if (newInvocation || Date.now() - record.restart.requestedAt > 60_000) {
          const loaded = newInvocation && process.env.PARASAUR_WORKER_RELEASE === record.restart.bundle
            && (!record.target_revision || record.current_revision === record.target_revision)
            && process.execPath === resolve(this.cwd,'bin/node');
          record.status = loaded ? 'success' : 'failed';
          if (!loaded) record.output += "\n[gah] The worker did not load the selected release after restart.";
          record.finished_at = new Date().toISOString();
          delete record.restart;
          writeState(record);
        }
      }
      // A `running` record whose updater is gone recorded no exit code:
      // unlike central, a worker update never restarts this server as its
      // own final step (the macOS restart happens after success is
      // recorded), so a dead pid here means the updater was killed.
      if (record.status === 'running' && record.pid !== null && record.pid !== this.updaterPid && !pidAlive(record.pid)) {
        record.status = 'failed';
        record.finished_at = new Date().toISOString();
        record.output = `${record.output}\n[gah] the updater process disappeared without recording an exit`.slice(-MAX_OUTPUT_CHARS);
        writeState(record);
      }
      return record;
    } catch {
      return { ...idleStatus(), pid: null };
    }
  }

  status(): WorkerUpdateStatus {
    return view(this.read());
  }

  draining(): boolean {
    if (this.starting) return true;
    const status = this.read().status;
    return status === 'waiting' || status === 'running' || status === 'inferred_restart';
  }

  async start(expected?: string): Promise<{ started: boolean; status: WorkerUpdateStatus }> {
    if (this.starting) {
      const result = await this.starting;
      return {started:false,status:result.status};
    }
    this.starting = this.startOnce(expected);
    try { return await this.starting; }
    finally { this.starting = undefined; }
  }

  private async startOnce(expected?: string): Promise<{ started: boolean; status: WorkerUpdateStatus }> {
    const existing = this.read();
    if ((existing.status === 'inferred_restart' && existing.restart) || (existing.status === 'running' && existing.pid !== null)) {
      return { started: false, status: view(existing) };
    }
    if (existing.status === 'waiting') {
      this.scheduleDrain();
      return { started: true, status: view(existing) };
    }
    const selected = expected === undefined ? undefined : await selectReleaseManifest(expected);
    const active = await this.countClaims();
    if (active !== 0) {
      const armed: WorkerUpdateRecord = {
        ...idleStatus(),
        status: 'waiting',
        armed_at: new Date().toISOString(),
        active_dispatches: active,
        target_revision: expected,
        selectedManifest: selected,
        pid: null
      };
      writeState(armed);
      this.scheduleDrain();
      return { started: true, status: view(armed) };
    }
    this.launch(selected, expected);
    return { started: true, status: this.status() };
  }

  /** Cancels an armed (waiting) update. Anything further along runs to
   * completion -- a downloaded update must not be half-cancelled. */
  cancel(): boolean {
    const existing = this.read();
    if (existing.status !== 'waiting') return false;
    this.stopDrain();
    writeState({ ...idleStatus(), pid: null });
    return true;
  }

  /** Null when the claims cannot be read: callers arm (or stay armed)
   * instead of launching, so an unreadable claim list never restarts a
   * worker mid-dispatch. */
  private async countClaims(): Promise<number | null> {
    try {
      return await (this.deps.countActiveClaims ?? countActiveDispatches)();
    } catch {
      return null;
    }
  }

  private scheduleDrain(): void {
    if (this.drainTimer) return;
    this.drainTimer = setInterval(() => {
      void this.drainTick().catch(() => undefined);
    }, DRAIN_POLL_MS);
    this.drainTimer.unref?.();
  }

  private stopDrain(): void {
    if (this.drainTimer) {
      clearInterval(this.drainTimer);
      this.drainTimer = null;
    }
  }

  private async drainTick(): Promise<void> {
    const current = this.read();
    if (current.status !== 'waiting') {
      this.stopDrain();
      return;
    }
    const active = await this.countClaims();
    const latest = this.read();
    if (latest.status !== 'waiting' || latest.armed_at !== current.armed_at) return;
    if (active !== 0) {
      writeState({ ...current, active_dispatches: active });
      return;
    }
    this.stopDrain();
    this.launch(current.selectedManifest, current.target_revision);
  }

  private launch(selected?: string, revision?: string | null): void {
    let child: ChildProcess;
    try {
      if (revision && (!selected || buildCommit(JSON.parse(selected).commit) !== revision)) {
        throw new Error('The saved manifest for the selected worker build is unavailable.');
      }
      const manifest = selected ? persistReleaseManifest(workerUpdateStatePath(), selected) : process.env.PARASAUR_RELEASE_MANIFEST;
      child = this.spawnFn(findGahBinary(), [
        'update', '--from-release', '--role', 'worker', '--repo', this.cwd,
        ...(manifest ? ['--release-manifest', manifest] : [])
      ], {
        cwd: this.cwd,
        env: process.env,
        detached: true,
        stdio: ['ignore', 'pipe', 'pipe']
      });
    } catch (error) {
      writeState({ ...idleStatus(), status: 'failed', pid: null, target_revision: revision,
        finished_at: new Date().toISOString(), output: error instanceof Error ? error.message : 'Could not start the worker update.' });
      return;
    }
    child.unref();
    this.updaterPid = child.pid;

    let record: WorkerUpdateRecord = {
      ...idleStatus(),
      status: 'running',
      target_revision: revision,
      started_at: new Date().toISOString(),
      pid: child.pid ?? null
    };
    writeState(record);

    const appendOutput = (chunk: Buffer | string) => {
      record = {
        ...record,
        output: (record.output + chunk.toString()).slice(-MAX_OUTPUT_CHARS)
      };
      writeState(record);
    };
    child.stdout?.on('data', appendOutput);
    child.stderr?.on('data', appendOutput);

    child.on('close', (code) => {
      this.updaterPid = undefined;
      // A reservation can win after the poll but before the installer's
      // exclusive maintenance check. Keep the original release selection.
      if (code === 75) {
        writeState({ ...record, status: 'waiting', pid: null, started_at: null, armed_at: new Date().toISOString(),
          active_dispatches: null, selectedManifest: selected });
        this.scheduleDrain();
        return;
      }
      record = {
        ...record,
        status: code === 0 ? 'success' : 'failed',
        finished_at: new Date().toISOString(),
        active_dispatches: null
      };
      if (code === 0 && process.platform === 'linux' && this.isInstalledWslWorker()) {
        try {
          const selection = readFileSync(resolve(dirname(this.cwd),'release.env'),'utf8');
          const bundle = /^export PARASAUR_WORKER_RELEASE=([a-f0-9]{64})$/m.exec(selection)?.[1];
          const previousInvocation = process.env.INVOCATION_ID;
          if (!bundle || !previousInvocation) throw Error('Cannot identify the installed release or service');
          record = {...record,status:'inferred_restart',finished_at:null,restart:{previousInvocation,bundle,requestedAt:Date.now()}};
          writeState(record);
          if (this.deps.restartWorker) this.deps.restartWorker();
          else this.restartWslWorker();
        } catch {
          record = {...record,status:'failed',finished_at:new Date().toISOString(),output:`${record.output}\n[gah] The release was installed, but systemd could not restart this worker's service.`.slice(-MAX_OUTPUT_CHARS)};
          writeState(record);
        }
      } else {
        writeState(record);
        if (code === 0 && process.platform === 'darwin') this.restartWorker();
      }
    });

    child.on('error', (error) => {
      this.updaterPid = undefined;
      record = {
        ...record,
        status: 'failed',
        finished_at: new Date().toISOString(),
        output: `${record.output}\n[gah] failed to spawn the updater: ${error.message}`.slice(-MAX_OUTPUT_CHARS)
      };
      writeState(record);
    });
  }

  private isInstalledWslWorker(): boolean {
    const root = resolve(homedir(), '.local/share/gah/worker');
    return dirname(this.cwd) === root && basename(this.cwd).startsWith('release.') && existsSync(resolve(root, 'release.env'));
  }

  /** Restart only the user service whose main process is this worker. A
   * foreground process or another instance must not restart a sibling. */
  private restartWslWorker(): void {
    const systemctl = (...args: string[]) => execFileSync('/usr/bin/systemctl', ['--user', ...args], {
      encoding:'utf8', stdio:['ignore','pipe','pipe'], timeout:5000
    }).trim();
    const properties = systemctl('show', '--property=Id,MainPID,InvocationID', '--', 'gah-worker*.service');
    const unit = workerServiceUnit(properties, process.pid, process.env.INVOCATION_ID);
    // The manager acknowledges this timer before it stops our cgroup.
    // Otherwise it can kill the systemctl client that is returning success.
    execFileSync('/usr/bin/systemd-run', ['--user','--quiet','--collect','--on-active=1s','--timer-property=AccuracySec=100ms',
      '/usr/bin/systemctl','--user','restart',unit], {stdio:'ignore',timeout:5000});
  }

  /** Restart this worker's LaunchAgent so the node server runs the release
   * it just installed (the success state is already on disk for the freshly
   * started process -- and central -- to read). No-op where the worker is
   * not launchd-managed: the artifacts are installed and the next manual
   * restart picks them up. */
  private restartWorker(): void {
    if (this.deps.restartWorker) {
      this.deps.restartWorker();
      return;
    }
    const script = resolve(this.cwd, 'scripts/macos-launchd.sh');
    if (!existsSync(script)) return;
    try {
      const restart = spawn('bash', [script, 'start', 'worker'], {
        cwd: this.cwd,
        detached: true,
        stdio: 'ignore'
      });
      restart.unref();
    } catch {
      // The restart is best-effort; the install already succeeded.
    }
  }
}
