/**
 * Central claim arbitration (issue #882): the shared exclusivity gate that
 * lets `gah dispatch`/`gah loop` on different nodes safely avoid picking up
 * the same work_id twice. Replaces (only when `registry_central_url` is
 * configured -- see src/central_claims.rs on the Rust side) the local
 * ledger-claim mechanism's cross-node role; the *local* PID+file lock
 * (work_claim.rs) is untouched, it protects a different, single-machine
 * problem.
 *
 * Storage is a private JSON file + in-memory Map, not SQLite. Writes replace
 * the file atomically, and synchronous mutations cannot interleave within one
 * server process. Independently loaded service processes remain explicitly
 * last-write-wins; the deployment contract runs one central claims service.
 */

import { readFileSync, existsSync } from 'node:fs';
import { resolve } from 'node:path';
import type { ClaimLease, NativeClaimBinding, NativeClaimHold, NativeResultAcknowledgement, NativeReservationRelease } from '@git-agent-harness/contracts';
import { writeFileAtomic } from './atomicFile.js';

const DEFAULT_LEASE_SECONDS = 15 * 60; // 15 min; renewal interval is lease/3, see central_claims.rs
const MIN_LEASE_SECONDS = 60;
const MAX_LEASE_SECONDS = 60 * 60;

export class ClaimConflictError extends Error {
  constructor(public readonly heldBy: ClaimLease) {
    super(heldBy.native_handoff
      ? `work_id '${heldBy.work_id}' on profile '${heldBy.profile}' remains held for native run '${heldBy.native_handoff.run_id}' on worker '${heldBy.native_handoff.node_id}' (${heldBy.native_handoff.phase}); its canonical result must be reconciled`
      : `work_id '${heldBy.work_id}' on profile '${heldBy.profile}' is already claimed by node '${heldBy.node_id}'`);
  }
}

function claimKey(profile: string, workId: string): string {
  return JSON.stringify([profile, canonicalWorkId(workId)]);
}

/** Same issue aliases as Rust work_claim::normalize_work_identity. */
export function canonicalWorkId(value: string): string {
  const trimmed = value.trim();
  const digits = trimmed.startsWith('TICKET-') ? trimmed.slice(7) : trimmed.startsWith('#') ? trimmed.slice(1) : trimmed;
  return /^[0-9]+$/.test(digits) ? `#${digits.replace(/^0+/, '') || '0'}` : trimmed;
}

const digest = (value: unknown): value is string => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
function validNative(binding: NativeClaimBinding): boolean {
  return typeof binding.run_id === 'string' && typeof binding.node_id === 'string'
    && /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(binding.run_id)
    && binding.run_id !== '00000000-0000-0000-0000-000000000000'
    && /^[A-Za-z0-9_-]{1,128}$/.test(binding.node_id) && digest(binding.context_sha256);
}
const sameNative = (left: NativeClaimBinding | undefined, right: NativeClaimBinding) => left?.run_id === right.run_id
  && left.node_id === right.node_id && left.context_sha256 === right.context_sha256;
const copyLease = (lease: ClaimLease): ClaimLease => ({ ...lease, ...(lease.native_handoff ? { native_handoff: { ...lease.native_handoff } } : {}) });

function clampLeaseSeconds(requested: number | undefined): number {
  const value = requested ?? DEFAULT_LEASE_SECONDS;
  return Math.min(MAX_LEASE_SECONDS, Math.max(MIN_LEASE_SECONDS, value));
}

export class ClaimsService {
  private configPath: string;
  private leases: Map<string, ClaimLease> = new Map();
  private persistenceUncertain = false;

  constructor(configPath?: string, private readonly now: () => number = Date.now) {
    this.configPath = configPath || process.env.GAH_CLAIMS_CONFIG_PATH || resolve(process.cwd(), 'config/claims-config.json');
    this.load();
  }

  private load() {
    if (existsSync(this.configPath)) {
      try {
        const data = JSON.parse(readFileSync(this.configPath, 'utf8'));
        if (data.version !== undefined && data.version !== 1 && data.version !== 2) throw new Error('Unsupported claims store version');
        if (data.version === 2 && data.leases !== undefined) throw new Error('Ambiguous fenced claims store');
        const leases = data.version === 2 ? data.fenced_leases : data.leases;
        if (!Array.isArray(leases)) {
          throw new Error('expected a leases array');
        }
        for (const lease of leases as ClaimLease[]) {
          if (lease.native_handoff !== undefined && data.version !== 2) throw new Error('Native claims require a fenced store');
          if (lease.native_handoff !== undefined && (!validNative(lease.native_handoff)
            || !['reserved', 'offered', 'running', 'reconciling', 'result_pending'].includes(lease.native_handoff.phase)
            || !Number.isFinite(Date.parse(lease.native_handoff.held_at))
            || !Number.isFinite(Date.parse(lease.expires_at)))) throw new Error('Invalid native claim hold');
          const key = claimKey(lease.profile, lease.work_id);
          if (this.leases.has(key)) throw new Error('Duplicate canonical claim identity');
          this.leases.set(key, lease);
        }
      } catch (e) {
        throw new Error(
          `Failed to load claims config ${this.configPath}: ${e instanceof Error ? e.message : String(e)}`,
          { cause: e }
        );
      }
    }
  }

  private healthy() {
    if (this.persistenceUncertain) throw new Error('Claims persistence is uncertain; reload the claims service after inspecting its saved state.');
  }

  /** Publish the in-memory state only after file and directory durability.
   * An error after rename has an unknown commit outcome, so this instance
   * refuses further operations rather than granting from its older map. */
  private commit(next: Map<string, ClaimLease>) {
    this.healthy();
    try {
      // The shared writer fsyncs its writable temporary handle (also required
      // by Windows FlushFileBuffers). Native Linux requires directory fsync;
      // Windows has no supported directory-flush API here.
      const leases = [...next.values()];
      // Older APIs only accept `leases`. They must fail closed on rollback
      // while remote ownership is unresolved, rather than expiring its fence.
      const stored = leases.some(lease => lease.native_handoff) ? { version: 2, fenced_leases: leases } : { leases };
      writeFileAtomic(this.configPath, `${JSON.stringify(stored, null, 2)}\n`, 0o600, process.platform !== 'win32');
      this.leases = next;
    } catch (e) {
      this.persistenceUncertain = true;
      console.error('Failed to save claims config:', e);
      throw e;
    }
  }

  /** Deletes any lease past its expiry -- opportunistic GC piggybacked on
   * real traffic instead of a separate sweep timer. Not called for its own
   * sake; every mutating call below runs it first so reads never have to
   * special-case a stale entry. */
  private sweepExpired(nowMs: number): void {
    this.healthy();
    for (const [key, lease] of this.leases) {
      if (!lease.native_handoff && Date.parse(lease.expires_at) <= nowMs) {
        this.leases.delete(key);
      }
    }
  }

  /** Synchronous by design -- see module docs. Throws ClaimConflictError
   * (caller maps to HTTP 409) when a different node currently holds an
   * unexpired lease. Re-acquiring your own already-held claim is treated as
   * idempotent (refreshes the lease) rather than a conflict, since a worker
   * retrying its own acquire call after a network blip is the common case,
   * not an error. */
  acquire(nodeId: string, profile: string, workId: string, leaseSeconds?: number): ClaimLease {
    const now = this.now();
    this.sweepExpired(now);
    const key = claimKey(profile, workId);
    const existing = this.leases.get(key);
    if (existing && (existing.node_id !== nodeId || existing.native_handoff)) {
      throw new ClaimConflictError(existing);
    }
    const nowIso = new Date(now).toISOString();
    const lease: ClaimLease = {
      profile,
      work_id: workId,
      node_id: nodeId,
      claimed_at: existing?.claimed_at ?? nowIso,
      renewed_at: nowIso,
      expires_at: new Date(now + clampLeaseSeconds(leaseSeconds) * 1000).toISOString()
    };
    const next = new Map(this.leases); next.set(key, lease); this.commit(next);
    return copyLease(lease);
  }

  /** Throws ClaimConflictError if the lease doesn't exist, expired, or is
   * held by a different node -- a renewal can't resurrect a lease that
   * already lapsed and may have been picked up by someone else. */
  renew(nodeId: string, profile: string, workId: string, leaseSeconds?: number): ClaimLease {
    const now = this.now();
    this.sweepExpired(now);
    const key = claimKey(profile, workId);
    const existing = this.leases.get(key);
    if (!existing || existing.node_id !== nodeId || existing.native_handoff) {
      throw new ClaimConflictError(
        existing ?? { profile, work_id: workId, node_id: '(none)', claimed_at: '', renewed_at: '', expires_at: '' }
      );
    }
    const nowIso = new Date(now).toISOString();
    const lease: ClaimLease = {
      ...existing,
      renewed_at: nowIso,
      expires_at: new Date(now + clampLeaseSeconds(leaseSeconds) * 1000).toISOString()
    };
    const next = new Map(this.leases); next.set(key, lease); this.commit(next);
    return copyLease(lease);
  }

  /** No-op (not an error) if the lease is already gone or held by someone
   * else -- release is best-effort cleanup, not a claim you can fight over. */
  release(nodeId: string, profile: string, workId: string): void {
    this.healthy();
    const key = claimKey(profile, workId);
    const existing = this.leases.get(key);
    if (existing && existing.node_id === nodeId && !existing.native_handoff) {
      const next = new Map(this.leases); next.delete(key); this.commit(next);
    }
  }

  /** Current leases and unresolved native holds. An expired native lease
   * still blocks competing work, but does not authorize another backend start. */
  getLeases(): ClaimLease[] {
    this.sweepExpired(this.now());
    return [...this.leases.values()].map(copyLease);
  }

  getLease(profile: string, workId: string): ClaimLease | undefined {
    this.sweepExpired(this.now());
    const lease = this.leases.get(claimKey(profile, workId));
    return lease ? copyLease(lease) : undefined;
  }

  /** Reserve a prepared central run before offering execution. No legacy
   * lease, including this coordinator's, can be silently adopted or replaced.
   * Exact retries retain the original hold and expiry; this is not a launch
   * authorization or a lease renewal. */
  reserveNative(owner: string, profile: string, workId: string, binding: NativeClaimBinding): ClaimLease {
    if (!validNative(binding) || ![owner, profile, workId].every(value => typeof value === 'string'
      && value.trim().length > 0 && value.length <= 4096 && !/[\x00-\x1f\x7f]/.test(value))) throw new Error('Invalid native claim binding');
    const now = this.now(); this.sweepExpired(now);
    const key = claimKey(profile, workId); const existing = this.leases.get(key);
    if (existing) {
      if (existing.node_id === owner && sameNative(existing.native_handoff, binding)) {
        // A prior rename may have succeeded without a confirmed directory
        // sync. Replayed acknowledgements must establish durability again.
        this.commit(new Map(this.leases));
        return copyLease(existing);
      }
      throw new ClaimConflictError(existing);
    }
    // A run ID cannot acquire a second work identity or executing node.
    const collision = [...this.leases.values()].find(lease => lease.native_handoff?.run_id === binding.run_id);
    if (collision) throw new ClaimConflictError(collision);
    const at = new Date(now).toISOString();
    const lease: ClaimLease = { node_id: owner, profile, work_id: workId, claimed_at: at, renewed_at: at,
      expires_at: new Date(now + DEFAULT_LEASE_SECONDS * 1000).toISOString(),
      native_handoff: { ...binding, phase: 'reserved', held_at: at } };
    const next = new Map(this.leases); next.set(key, lease); this.commit(next);
    return copyLease(lease);
  }

  /** Record a write-ahead offer intent or an observed lifecycle change.
   * None of these observations creates execution authority. */
  observeNative(owner: string, profile: string, workId: string, binding: NativeClaimBinding, phase: 'offered' | 'running' | 'reconciling'): ClaimLease {
    this.healthy();
    if (!['offered', 'running', 'reconciling'].includes(phase)) throw new Error('Invalid native observation phase');
    const key = claimKey(profile, workId); const existing = this.leases.get(key);
    if (!existing || existing.node_id !== owner || !sameNative(existing.native_handoff, binding)) {
      throw new ClaimConflictError(existing ?? { profile, work_id: workId, node_id: '(none)', claimed_at: '', renewed_at: '', expires_at: '' });
    }
    if (phase === 'offered' && !['reserved', 'offered'].includes(existing.native_handoff!.phase)) {
      throw new Error('Native offer already advanced; reconcile the existing run');
    }
    const lease: ClaimLease = { ...existing, native_handoff: { ...existing.native_handoff!, phase } };
    const next = new Map(this.leases); next.set(key, lease); this.commit(next);
    return copyLease(lease);
  }

  /** Only the trusted broker calls this after fencing out every start and
   * durably recording the original worker's physical-release proof. Possible
   * execution still requires canonical result import. Never expose as RPC. */
  cancelNativeReservation(owner: string, profile: string, workId: string, binding: NativeClaimBinding,
    reservationId: string, proof: NativeReservationRelease): void {
    this.healthy();
    if (!validNative(binding) || !proof || !sameNative(proof,binding) || !reservationId
      || proof.reservation_id!==reservationId || proof.capacity_released!==true || proof.dispatch_started!==false) {
      throw Error('Unused native reservation release is unconfirmed');
    }
    const key=claimKey(profile,workId); const existing=this.leases.get(key);
    if (existing?.node_id===owner && sameNative(existing.native_handoff,binding)) {
      if (['running','result_pending'].includes(existing.native_handoff!.phase)) throw Error('Native execution requires canonical result reconciliation');
      const next=new Map(this.leases); next.delete(key); this.commit(next);
    }
  }

  /** The central broker supplies its canonical ledger importer, never a
   * worker success boolean. Persist pending-result ownership before awaiting
   * I/O; release only this exact fence after a matching durable acknowledgement.
   * A late replay may acknowledge its old result but cannot release newer work. */
  async completeNative(owner: string, profile: string, workId: string, binding: NativeClaimBinding,
    recordCanonicalResult: () => Promise<NativeResultAcknowledgement>): Promise<NativeResultAcknowledgement> {
    this.healthy();
    if (!validNative(binding)) throw new Error('Invalid native claim binding');
    const key = claimKey(profile, workId); const existing = this.leases.get(key);
    if (existing?.node_id === owner && sameNative(existing.native_handoff, binding)) {
      const next = new Map(this.leases);
      next.set(key, { ...existing, native_handoff: { ...existing.native_handoff!, phase: 'result_pending' } });
      this.commit(next);
    }
    const ack = await recordCanonicalResult();
    if (!sameNative(ack, binding) || ack.profile !== profile || typeof ack.work_id !== 'string'
      || canonicalWorkId(ack.work_id) !== canonicalWorkId(workId)
      || !digest(ack.receipt_sha256) || !['recorded', 'already_recorded'].includes(ack.outcome)) {
      throw new Error('Canonical native result acknowledgement does not match the held run');
    }
    this.healthy();
    const current = this.leases.get(key);
    if (current?.node_id === owner && sameNative(current.native_handoff, binding)) {
      const next = new Map(this.leases); next.delete(key); this.commit(next);
    }
    return { ...ack };
  }
}
