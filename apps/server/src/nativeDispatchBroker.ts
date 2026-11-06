/** Central native dispatch orchestration. One service owns this root; completed
 * runs stay on disk rather than in an ever-growing in-memory receipt map. */
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';
import { closeSync, constants, existsSync, fstatSync, fsyncSync, lstatSync, mkdirSync, openSync, readFileSync, readdirSync, unlinkSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import type { NativeClaimBinding, NativeResultAcknowledgement } from '@git-agent-harness/contracts';
import type { ClaimsService } from './claimsService.js';
import { writeFileAtomic } from './atomicFile.js';

export interface NativeDispatchBinding extends NativeClaimBinding {
  profile: string; repo_id: string; repo: string; work_id: string; mode: string;
  backend: string; backend_instance: string | null; model: string | null;
  branch: string | null; review_generation: string | null; dispatch_reason: string | null;
}
export interface NativeHostReservation extends NativeClaimBinding {
  id: string; expires_at: string;
}
/** A short-lived permission to start the verified job wrapper, not blanket
 * permission for subsequent backend attempts or publication. */
export interface NativeStartGrant extends NativeClaimBinding {
  reservation_id: string; expires_at: string; token: string;
}
const authorityReasons = ['factory_paused','controller_changed','policy_changed','budget_held','claim_lost','authority_unavailable','authority_busy'] as const;
export type NativeAuthorization = { allowed: true }
  | { allowed: false; reason: typeof authorityReasons[number] };
export type NativeGrantConsumption = (NativeClaimBinding & { allowed: true; reservation_id: string; expires_at: string; consumed_at: string })
  | { allowed: false; reason: 'invalid_grant' | 'expired' | 'already_consumed' | 'consumption_in_progress' | Exclude<NativeAuthorization, {allowed:true}>['reason'] };
export type NativeWorkerObservation = NativeClaimBinding & (
  { state: 'waiting_capacity' } | { state: 'ready'; reservation: NativeHostReservation }
  | { state: 'running' } | { state: 'unknown' } | { state: 'terminal'; receipt_json: string }
);
export interface NativeWorkerTransport {
  stage(binding: NativeDispatchBinding, contextDirectory: string, signal?: AbortSignal): Promise<NativeWorkerObservation>;
  start(binding: NativeDispatchBinding, grant: NativeStartGrant, signal?: AbortSignal): Promise<NativeWorkerObservation>;
  observe(binding: NativeDispatchBinding, signal?: AbortSignal): Promise<NativeWorkerObservation>;
  releaseReservation(binding: NativeDispatchBinding, reservation: NativeHostReservation, signal?: AbortSignal): Promise<void>;
}
type Phase = 'prepared' | 'reserved' | 'stage_offered' | 'waiting_capacity' | 'authorization_deferred'
  | 'start_offered' | 'running' | 'reconciling' | 'result_pending' | 'terminal';
interface Journal {
  version: 1; binding: NativeDispatchBinding; binding_sha256: string; phase: Phase; stage_offered: boolean; start_offered: boolean; terminal_observed: boolean;
  updated_at: string; reason?: string; receipt_file?: string; receipt_sha256?: string;
  acknowledgement?: NativeResultAcknowledgement; history_cleaned?: boolean;
  start_grant?: { reservation: NativeHostReservation; issued_at: string; expires_at: string; token_sha256: string; consumed_at?: string };
}
export type NativeDispatchView = Pick<Journal, 'phase' | 'updated_at' | 'reason' | 'acknowledgement'> & { binding: NativeDispatchBinding; grant_consumed_at?: string };
type Deps = {
  root: string; centralUrl: string; coordinatorNodeId: string; claims: ClaimsService; transport: NativeWorkerTransport;
  authorizeStart: (binding: NativeDispatchBinding, reservation: NativeHostReservation, signal?: AbortSignal) => Promise<NativeAuthorization>;
  recordCanonicalResult: (files: { bindingFile: string; receiptFile: string }) => Promise<NativeResultAcknowledgement>;
  now?: () => number;
  observationTimeoutMs?: number;
};
const MAX_JSON = 8 * 1024 * 1024;
const hash = (bytes: string | Buffer) => createHash('sha256').update(bytes).digest('hex');
const same = (left: NativeClaimBinding, right: NativeClaimBinding) => left?.run_id === right.run_id && left.node_id === right.node_id && left.context_sha256 === right.context_sha256;
export const isNativeRunId = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(value) && value !== '00000000-0000-0000-0000-000000000000';
const digest = (value: string) => /^[a-f0-9]{64}$/.test(value);
const phases: Phase[] = ['prepared', 'reserved', 'stage_offered', 'waiting_capacity', 'authorization_deferred', 'start_offered', 'running', 'reconciling', 'result_pending', 'terminal'];
const bindingFields = ['run_id', 'node_id', 'context_sha256', 'profile', 'repo_id', 'repo', 'work_id', 'mode', 'backend', 'backend_instance', 'model', 'branch', 'review_generation', 'dispatch_reason'] as const;
const bindingHash = (binding: NativeDispatchBinding) => hash(JSON.stringify(bindingFields.map(key => binding[key])));

function syncDirectory(path: string): void {
  if (process.platform === 'win32') return;
  const file = openSync(path, constants.O_RDONLY);
  try { fsyncSync(file); } finally { closeSync(file); }
}
function directory(path: string): void {
  const info = lstatSync(path);
  if (!info.isDirectory() || info.isSymbolicLink() || (process.platform !== 'win32'
    && (info.uid !== process.getuid!() || (info.mode & 0o022) !== 0))) throw Error('Native broker directory is not owned and protected');
}
function readPrivate(path: string): Buffer {
  const file = openSync(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const info = fstatSync(file);
    if (!info.isFile() || info.size > MAX_JSON || (process.platform !== 'win32' && (info.uid !== process.getuid!() || (info.mode & 0o077) !== 0))) throw Error('Native broker input is not a private bounded file');
    return readFileSync(file);
  } finally { closeSync(file); }
}
const readJson = <T>(path: string): T => JSON.parse(readPrivate(path).toString('utf8')) as T;
function view(journal: Journal): NativeDispatchView {
  return structuredClone({ binding: journal.binding, phase: journal.phase, updated_at: journal.updated_at,
    ...(journal.start_grant?.consumed_at ? { grant_consumed_at: journal.start_grant.consumed_at } : {}),
    ...(journal.reason ? { reason: journal.reason } : {}), ...(journal.acknowledgement ? { acknowledgement: journal.acknowledgement } : {}) });
}

export class NativeDispatchBroker {
  private readonly root: string;
  private readonly now: () => number;
  private readonly active = new Map<string, Promise<NativeDispatchView>>();
  private readonly consuming = new Set<string>();
  private persistenceUncertain = false;
  private readonly timeout: number;

  constructor(private readonly deps: Deps) {
    this.root = resolve(deps.root); this.now = deps.now ?? Date.now;
    this.timeout = deps.observationTimeoutMs ?? 5000;
    if (!Number.isInteger(this.timeout) || this.timeout < 1 || this.timeout > 60_000) throw Error('Invalid native observation timeout');
    if (!existsSync(this.root)) { mkdirSync(this.root, { recursive: true, mode: 0o700 }); syncDirectory(dirname(this.root)); }
    directory(this.root);
    // The run journal must survive loss of its newly-created ancestor entries,
    // not merely an fsync of the journal's immediate directory.
    if (process.platform !== 'win32') for (let parent = dirname(this.root);;) {
      syncDirectory(parent); const next = dirname(parent); if (next === parent) break; parent = next;
    }
    if (process.platform !== 'win32' && (lstatSync(this.root).mode & 0o077) !== 0) throw Error('Native broker root must be private');
  }

  /** The context must already have been prepared by the trusted central CLI in
   * root/<run UUID>/context. Changed requests/policies produce another digest
   * and cannot replace this run. No caller supplies an arbitrary binding path. */
  offerPrepared(runId: string, contextSha256: string): Promise<NativeDispatchView> {
    if (!isNativeRunId(runId) || !digest(contextSha256)) return Promise.reject(Error('Invalid native run binding'));
    return this.serial(runId, async () => {
      const prepared = this.prepared(runId, contextSha256);
      const path = this.journalPath(runId);
      if (!existsSync(path) && this.deps.claims.getLeases().some(lease => lease.native_handoff?.run_id === runId)) {
        throw Error('Native ownership exists without its broker journal; retain the run for reconciliation');
      }
      let journal = existsSync(path) ? this.read(runId) : this.create(prepared);
      if (journal.binding_sha256 !== bindingHash(prepared)) throw Error('Native offer conflicts with its durable run binding');
      if (journal.phase === 'terminal') return view(this.cleanHistory(journal));
      if (journal.stage_offered) return this.reconcileRun(journal);
      this.deps.claims.reserveNative(this.deps.coordinatorNodeId, prepared.profile, prepared.work_id, prepared);
      journal = this.update(journal, { phase: 'reserved' });
      // Both durable intent records precede the first transport side effect.
      this.deps.claims.observeNative(this.deps.coordinatorNodeId, prepared.profile, prepared.work_id, prepared, 'offered');
      journal = this.update(journal, { phase: 'stage_offered', stage_offered: true });
      try { return await this.observed(journal, await this.deadline(signal => this.deps.transport.stage(prepared, this.contextPath(runId), signal))); }
      catch { return this.uncertain(journal, 'stage_unconfirmed'); }
    });
  }

  /** Poll the same durable run. A missing roster item, timeout or restart is
   * never permission to repeat stage/start or invent a terminal result. */
  reconcile(runId: string): Promise<NativeDispatchView> {
    if (!isNativeRunId(runId)) return Promise.reject(Error('Invalid native run identity'));
    return this.serial(runId, () => this.reconcileRun(this.read(runId)));
  }
  get(runId: string): NativeDispatchView { return view(this.read(runId)); }

  /** A worker wrapper exchanges its secret before entering dispatch. This
   * cannot take the offer lock: transport.start may await this very callback.
   * After the asynchronous fresh check, re-read and commit synchronously so
   * only one caller can receive permission. A lost reply is never replayed as
   * permission; the wrapper must stop and retain its result for reconciliation.
   * This receipt grants no later attempt or publication authority. */
  async consumeStartGrant(grant: NativeStartGrant): Promise<NativeGrantConsumption> {
    if (!grant || !isNativeRunId(grant.run_id) || !digest(grant.context_sha256) || typeof grant.token !== 'string'
      || !/^[a-f0-9]{64}$/.test(grant.token)) return { allowed: false, reason: 'invalid_grant' };
    if (this.consuming.has(grant.run_id)) return { allowed: false, reason: 'consumption_in_progress' };
    this.consuming.add(grant.run_id);
    try {
      let journal = this.read(grant.run_id);
      const refusal = this.grantRefusal(journal, grant);
      if (refusal) return { allowed: false, reason: refusal };
      if (!this.owns(journal.binding)) return { allowed: false, reason: 'claim_lost' };
      let decision: NativeAuthorization;
      try { decision = await this.deadline(signal => this.deps.authorizeStart(journal.binding, journal.start_grant!.reservation, signal)); }
      catch { return { allowed: false, reason: 'authority_unavailable' }; }
      if (decision?.allowed !== true) return { allowed: false, reason: this.denialReason(decision) };
      // Fresh permission cannot revive an expired/consumed grant or replace
      // ownership that changed while policy/provider checks were running.
      journal = this.read(grant.run_id);
      const changed = this.grantRefusal(journal, grant);
      if (changed) return { allowed: false, reason: changed };
      if (!this.owns(journal.binding)) return { allowed: false, reason: 'claim_lost' };
      if (bindingHash(this.prepared(grant.run_id, grant.context_sha256)) !== journal.binding_sha256) return { allowed: false, reason: 'invalid_grant' };
      const consumedTime = this.now();
      if (consumedTime >= Date.parse(journal.start_grant!.expires_at)) return { allowed: false, reason: 'expired' };
      const consumed_at = new Date(consumedTime).toISOString();
      this.update(journal, { start_grant: { ...journal.start_grant!, consumed_at } });
      if (this.now() >= Date.parse(grant.expires_at)) return { allowed: false, reason: 'expired' };
      return { allowed: true, run_id: grant.run_id, node_id: grant.node_id, context_sha256: grant.context_sha256,
        reservation_id: grant.reservation_id, expires_at: grant.expires_at, consumed_at };
    } catch { return { allowed: false, reason: 'authority_unavailable' }; }
    finally { this.consuming.delete(grant.run_id); }
  }

  private denialReason(decision: NativeAuthorization): Exclude<NativeAuthorization, { allowed: true }>['reason'] {
    return decision?.allowed === false && authorityReasons.includes(decision.reason)
      ? decision.reason : 'authority_unavailable';
  }
  private owns(binding: NativeDispatchBinding): boolean {
    const held = this.deps.claims.getLease(binding.profile, binding.work_id);
    return held?.node_id === this.deps.coordinatorNodeId && !!held.native_handoff && same(held.native_handoff, binding);
  }
  private grantRefusal(journal: Journal, grant: NativeStartGrant): 'invalid_grant' | 'expired' | 'already_consumed' | undefined {
    const stored = journal.start_grant;
    if (!journal.start_offered || journal.phase === 'terminal' || journal.terminal_observed || !stored || !same(grant, journal.binding)
      || grant.reservation_id !== stored.reservation.id || grant.expires_at !== stored.expires_at
      || !timingSafeEqual(Buffer.from(hash(grant.token), 'hex'), Buffer.from(stored.token_sha256, 'hex'))) return 'invalid_grant';
    if (stored.consumed_at) return 'already_consumed';
    if (this.now() < Date.parse(stored.issued_at) || Date.parse(stored.expires_at) <= this.now() || Date.parse(stored.reservation.expires_at) <= this.now()) return 'expired';
  }

  /** Abort only the network observation. The recorded worker operation may
   * still have happened; deadlines never cancel or re-offer the native run. */
  private async deadline<T>(operation: (signal: AbortSignal) => Promise<T>): Promise<T> {
    const controller = new AbortController(); let timer: ReturnType<typeof setTimeout> | undefined;
    try {
      return await Promise.race([operation(controller.signal), new Promise<never>((_, reject) => {
        timer = setTimeout(() => { controller.abort(); reject(Error('Native observation deadline exceeded')); }, this.timeout);
      })]);
    } finally { if (timer) clearTimeout(timer); }
  }

  private serial(run: string, operation: () => Promise<NativeDispatchView>): Promise<NativeDispatchView> {
    if (this.persistenceUncertain) return Promise.reject(Error('Native broker persistence is uncertain; inspect and reload its journal'));
    const previous = this.active.get(run);
    const current = (previous ? previous.catch(() => undefined) : Promise.resolve()).then(() => {
      if (this.persistenceUncertain) throw Error('Native broker persistence is uncertain; inspect and reload its journal');
      return operation();
    });
    this.active.set(run, current);
    void current.finally(() => { if (this.active.get(run) === current) this.active.delete(run); }).catch(() => undefined);
    return current;
  }
  private runPath(run: string): string {
    if (!isNativeRunId(run)) throw Error('Invalid native run identity');
    const path = join(this.root, run); directory(path); return path;
  }
  private contextPath(run: string): string { const path = join(this.runPath(run), 'context'); directory(path); return path; }
  private journalPath(run: string): string { return join(this.runPath(run), 'broker.json'); }
  private prepared(run: string, expected: string): NativeDispatchBinding {
    const context = this.contextPath(run);
    const binding = readJson<NativeDispatchBinding>(join(context, 'binding.json'));
    const bytes = readPrivate(join(context, 'manifest.json'));
    const manifest = JSON.parse(bytes.toString('utf8')) as { version: number; run_id: string; node_id: string; profile: string; central_url: string; history_chunks: number; files: Record<string, { sha256: string }> };
    if (binding.run_id !== run || binding.context_sha256 !== expected || hash(bytes) !== expected
      || manifest.version !== 2 || manifest.run_id !== run || manifest.node_id !== binding.node_id || manifest.profile !== binding.profile
      || !Number.isInteger(manifest.history_chunks) || manifest.history_chunks < 1 || manifest.history_chunks > 8
      || manifest.central_url !== this.deps.centralUrl.replace(/\/$/, '')
      || Object.keys(binding).length !== bindingFields.length || bindingFields.some(key => !(key in binding))
      || !binding.profile || !binding.work_id || !binding.node_id || !binding.model || ['auto', 'default'].includes(binding.model.toLowerCase())
      || !['codex','claude','opencode','openhands','cloud-coder','hermes','vibe','agy','agy-main','agy-second'].includes(binding.backend)) throw Error('Prepared native context does not match the trusted offer');
    const requestBytes = readPrivate(join(context, 'request.json'));
    const policyBytes = readPrivate(join(context, 'policy.json'));
    if (hash(requestBytes) !== manifest.files['request.json']?.sha256 || hash(policyBytes) !== manifest.files['policy.json']?.sha256) throw Error('Prepared native request or policy changed');
    const args = (JSON.parse(requestBytes.toString('utf8')) as { args: Record<string, unknown> }).args;
    const fields = (JSON.parse(policyBytes.toString('utf8')) as { fields: Record<string, unknown> }).fields;
    const derived = { run_id: args.run_id, node_id: manifest.node_id, context_sha256: expected, profile: args.profile,
      repo_id: fields.repo_id, repo: fields.repo, work_id: args.work_id, mode: args.mode, backend: args.backend,
      backend_instance: args.backend_instance ?? null, model: args.model ?? null, branch: args.existing_branch ?? args.branch ?? null,
      review_generation: args.expected_review_generation ?? null, dispatch_reason: args.dispatch_reason ?? null } as NativeDispatchBinding;
    if (bindingHash(binding) !== bindingHash(derived)) throw Error('Prepared binding differs from its manifest-bound request');
    return binding;
  }
  private create(binding: NativeDispatchBinding): Journal {
    // Persist the run directory entry as well as the journal inside it.
    syncDirectory(this.root);
    const journal: Journal = { version: 1, binding, binding_sha256: bindingHash(binding), phase: 'prepared', stage_offered: false, start_offered: false, terminal_observed: false, updated_at: new Date(this.now()).toISOString() };
    this.write(journal); return journal;
  }
  private read(run: string): Journal {
    if (this.persistenceUncertain) throw Error('Native broker persistence is uncertain; inspect and reload its journal');
    const journal = readJson<Journal>(this.journalPath(run));
    const grant = journal.start_grant;
    if (journal.version !== 1 || journal.binding?.run_id !== run || !digest(journal.binding.context_sha256)
      || !phases.includes(journal.phase) || typeof journal.stage_offered !== 'boolean' || typeof journal.start_offered !== 'boolean'
      || typeof journal.terminal_observed !== 'boolean' || journal.binding_sha256 !== bindingHash(journal.binding)
      || (grant && (!journal.start_offered || !same(grant.reservation, journal.binding) || !grant.reservation.id || !digest(grant.token_sha256)
        || !Number.isFinite(Date.parse(grant.issued_at)) || !Number.isFinite(Date.parse(grant.expires_at)) || !Number.isFinite(Date.parse(grant.reservation.expires_at))
        || Date.parse(grant.expires_at) <= Date.parse(grant.issued_at) || Date.parse(grant.expires_at) - Date.parse(grant.issued_at) > 10_000
        || Date.parse(grant.expires_at) > Date.parse(grant.reservation.expires_at)
        || (grant.consumed_at !== undefined && (!Number.isFinite(Date.parse(grant.consumed_at))
          || Date.parse(grant.consumed_at) < Date.parse(grant.issued_at) || Date.parse(grant.consumed_at) >= Date.parse(grant.expires_at)))))
      || (journal.start_offered && !journal.stage_offered) || (journal.phase === 'terminal' && (!journal.terminal_observed || !journal.acknowledgement
        || !same(journal.acknowledgement, journal.binding) || journal.acknowledgement.profile !== journal.binding.profile
        || journal.acknowledgement.work_id !== journal.binding.work_id || !digest(journal.acknowledgement.receipt_sha256)
        || !['recorded','already_recorded'].includes(journal.acknowledgement.outcome)))) throw Error('Unsupported native broker journal; retain it for reconciliation');
    return journal;
  }
  private write(journal: Journal): void {
    if (this.persistenceUncertain) throw Error('Native broker persistence is uncertain; inspect and reload its journal');
    try { writeFileAtomic(this.journalPath(journal.binding.run_id), `${JSON.stringify(journal)}\n`, 0o600, process.platform !== 'win32'); }
    catch (error) { this.persistenceUncertain = true; throw error; }
  }
  private update(previous: Journal, changes: Partial<Journal>): Journal {
    const current = existsSync(this.journalPath(previous.binding.run_id)) ? this.read(previous.binding.run_id) : previous;
    if (current.binding_sha256 !== previous.binding_sha256) throw Error('Native run binding changed during reconciliation');
    if (current.phase === 'terminal') return current;
    const next = { ...current, ...changes, updated_at: new Date(this.now()).toISOString() };
    this.write(next); return next;
  }
  private uncertain(journal: Journal, reason: string): NativeDispatchView {
    const current = this.read(journal.binding.run_id);
    if (current.phase === 'terminal') return view(current);
    const held = this.deps.claims.getLease(current.binding.profile, current.binding.work_id);
    if (held?.node_id === this.deps.coordinatorNodeId && held.native_handoff && same(held.native_handoff, current.binding)) {
      this.deps.claims.observeNative(this.deps.coordinatorNodeId, current.binding.profile, current.binding.work_id, current.binding, 'reconciling');
    }
    return view(this.update(current, { phase: 'reconciling', reason }));
  }
  private async reconcileRun(journal: Journal): Promise<NativeDispatchView> {
    if (journal.phase === 'terminal') return view(this.cleanHistory(journal));
    if (!journal.stage_offered) return view(journal);
    try { return await this.observed(journal, await this.deadline(signal => this.deps.transport.observe(journal.binding, signal))); }
    catch {
      if (journal.receipt_file) return this.importResult(journal);
      return this.uncertain(journal, 'worker_unreachable');
    }
  }
  private async observed(previous: Journal, observation: NativeWorkerObservation): Promise<NativeDispatchView> {
    let journal = this.read(previous.binding.run_id); const binding = journal.binding;
    if (journal.phase === 'terminal') return view(journal);
    if (!same(observation, binding)) return this.uncertain(journal, 'worker_identity_mismatch');
    if (observation.state === 'terminal') return this.receiveResult(journal, observation.receipt_json);
    // Once termination is saved centrally, a stale live-worker observation
    // must not postpone canonical import or downgrade its recovery state.
    if (journal.terminal_observed && journal.receipt_file) return this.importResult(journal);
    if (observation.state === 'unknown') return journal.receipt_file ? this.importResult(journal) : this.uncertain(journal, 'worker_state_unconfirmed');
    if (observation.state === 'running') {
      if (!journal.start_offered) return this.uncertain(journal, 'execution_without_start_intent');
      if (!journal.start_grant?.consumed_at) return this.uncertain(journal, 'execution_without_start_authority');
      this.deps.claims.observeNative(this.deps.coordinatorNodeId, binding.profile, binding.work_id, binding, 'running');
      return view(this.update(journal, { phase: 'running', reason: undefined }));
    }
    // A stale ready/capacity observation cannot cause a second start.
    if (journal.start_offered || journal.terminal_observed) return journal.receipt_file ? this.importResult(journal) : this.uncertain(journal, 'start_unconfirmed');
    if (observation.state === 'waiting_capacity') return view(this.update(journal, { phase: 'waiting_capacity', reason: 'host_capacity' }));
    if (observation.state !== 'ready') return this.uncertain(journal, 'worker_state_unconfirmed');
    const reservation = observation.reservation;
    if (!same(reservation, binding) || typeof reservation.id !== 'string' || !reservation.id || reservation.id.length > 4096
      || /[\x00-\x1f\x7f]/.test(reservation.id) || typeof reservation.expires_at !== 'string' || reservation.expires_at.length > 64 || Date.parse(reservation.expires_at) <= this.now()
      || !Number.isFinite(Date.parse(reservation.expires_at))) return this.uncertain(journal, 'host_reservation_invalid');
    if (bindingHash(this.prepared(binding.run_id, binding.context_sha256)) !== journal.binding_sha256) return this.uncertain(journal, 'prepared_context_changed');
    let decision: NativeAuthorization;
    try { decision = await this.deadline(signal => this.deps.authorizeStart(binding, reservation, signal)); }
    catch { decision = { allowed: false, reason: 'authority_unavailable' }; }
    journal = this.read(binding.run_id);
    if (journal.phase === 'terminal') return view(journal);
    if (journal.start_offered) return this.reconcileRun(journal);
    if (decision?.allowed !== true || Date.parse(reservation.expires_at) <= this.now()) {
      try { await this.deadline(signal => this.deps.transport.releaseReservation(binding, reservation, signal)); } catch { /* host reservation still expires without authorizing a start */ }
      const reason = decision?.allowed ? 'host_reservation_expired' : this.denialReason(decision);
      return view(this.update(journal, { phase: 'authorization_deferred', reason }));
    }
    if (!this.owns(binding)) return this.uncertain(journal, 'claim_lost');
    if (bindingHash(this.prepared(binding.run_id, binding.context_sha256)) !== journal.binding_sha256) return this.uncertain(journal, 'prepared_context_changed');
    const issued = this.now();
    const issued_at = new Date(issued).toISOString();
    const until = Math.min(issued + 10_000, Date.parse(reservation.expires_at));
    if (until <= issued) return view(this.update(journal, { phase: 'authorization_deferred', reason: 'host_reservation_expired' }));
    const grant: NativeStartGrant = { run_id: binding.run_id, node_id: binding.node_id, context_sha256: binding.context_sha256,
      reservation_id: reservation.id, expires_at: new Date(until).toISOString(), token: randomBytes(32).toString('hex') };
    // The grant is passed only to the worker, never written into public state.
    journal = this.update(journal, { phase: 'start_offered', start_offered: true, reason: undefined,
      start_grant: { reservation: { run_id: binding.run_id, node_id: binding.node_id, context_sha256: binding.context_sha256,
        id: reservation.id, expires_at: reservation.expires_at }, issued_at, expires_at: grant.expires_at, token_sha256: hash(grant.token) } });
    if (until <= this.now()) return this.uncertain(journal, 'authority_expired_before_start');
    try { return await this.observed(journal, await this.deadline(signal => this.deps.transport.start(binding, grant, signal))); }
    catch { return this.uncertain(journal, 'start_unconfirmed'); }
  }
  private async receiveResult(journal: Journal, receipt: string): Promise<NativeDispatchView> {
    journal = this.update(journal, { terminal_observed: true });
    if (typeof receipt !== 'string' || Buffer.byteLength(receipt) > MAX_JSON) return this.uncertain(journal, 'worker_receipt_invalid');
    const name = `receipt-${hash(receipt)}.json`;
    const path = join(this.runPath(journal.binding.run_id), name);
    if (!existsSync(path)) {
      if (readdirSync(this.runPath(journal.binding.run_id)).filter(name => /^receipt-[a-f0-9]{64}\.json$/.test(name)).length >= 4) return this.uncertain(journal, 'worker_result_changed_repeatedly');
      try { writeFileAtomic(path, receipt, 0o600, process.platform !== 'win32'); }
      catch (error) { this.persistenceUncertain = true; throw error; }
    }
    const pending = this.update(journal, { phase: 'result_pending', receipt_file: name, receipt_sha256: hash(receipt), reason: undefined });
    return this.importResult(pending);
  }
  private async importResult(previous: Journal): Promise<NativeDispatchView> {
    const journal = this.read(previous.binding.run_id);
    if (journal.phase === 'terminal') return view(journal);
    if (!journal.receipt_file || !/^receipt-[a-f0-9]{64}\.json$/.test(journal.receipt_file)) throw Error('Native receipt spool is missing or unsupported');
    const path = join(this.runPath(journal.binding.run_id), journal.receipt_file);
    if (hash(readPrivate(path)) !== journal.receipt_sha256) return this.uncertain(journal, 'worker_receipt_changed');
    if (bindingHash(this.prepared(journal.binding.run_id, journal.binding.context_sha256)) !== journal.binding_sha256) return this.uncertain(journal, 'prepared_binding_changed');
    try {
      const acknowledgement = await this.deps.claims.completeNative(this.deps.coordinatorNodeId, journal.binding.profile, journal.binding.work_id,
        // Rust's acknowledgement digest identifies its canonical JSON payload;
        // receipt_sha256 above guards the exact spool bytes, including formatting.
        journal.binding, () => this.deps.recordCanonicalResult({ bindingFile: join(this.contextPath(journal.binding.run_id), 'binding.json'), receiptFile: path }));
      const terminal = this.update(journal, { phase: 'terminal', acknowledgement, reason: undefined });
      return view(this.cleanHistory(terminal));
    } catch { return view(this.update(journal, { phase: 'result_pending', reason: 'canonical_import_unconfirmed' })); }
  }

  /** Keep the compact request/policy/binding/receipt journal permanently, but
   * reclaim history copies only after the canonical result is acknowledged.
   * Fixed owned filenames only; never recurse or delete a run directory. */
  private cleanHistory(terminal: Journal): Journal {
    if (terminal.history_cleaned) return terminal;
    let reason: string | undefined;
    try {
      const context = this.contextPath(terminal.binding.run_id);
      const manifest = readJson<{ history_chunks: number }>(join(context, 'manifest.json'));
      if (!Number.isInteger(manifest.history_chunks) || manifest.history_chunks < 1 || manifest.history_chunks > 8) throw Error('Unsupported history count');
      for (let index = 0; index < manifest.history_chunks; index++) {
        const path = join(context, `history-${String(index).padStart(4, '0')}.jsonl`);
        if (!existsSync(path)) continue;
        const info = lstatSync(path);
        if (!info.isFile() || info.isSymbolicLink() || info.nlink !== 1 || (process.platform !== 'win32'
          && (info.uid !== process.getuid!() || (info.mode & 0o077) !== 0))) throw Error('History is not an owned private file');
        unlinkSync(path);
      }
      syncDirectory(context);
    } catch { reason = 'history_cleanup_pending'; }
    const current = this.read(terminal.binding.run_id);
    const updated = { ...current, history_cleaned: !reason, reason };
    this.write(updated); return updated;
  }
}
