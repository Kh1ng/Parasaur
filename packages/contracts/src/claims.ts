// Issue #882: central claim arbitration. Distinct from ActiveClaim
// (gah.ts) -- that's a snapshot of a node's *local* work_claim.rs
// PID+file locks, surfaced for the advisory fleet preflight (#881). A
// ClaimLease is the actual cross-node exclusivity grant, held on the
// central node.

export interface ClaimAcquireRequest {
  node_id: string;
  profile: string;
  work_id: string;
  /** Seconds until the lease expires without a renewal. Server clamps to a
   * sane range; omit to use the server default. */
  lease_seconds?: number;
}

export interface ClaimLease {
  profile: string;
  work_id: string;
  node_id: string;
  claimed_at: string;
  renewed_at: string;
  expires_at: string;
  /** A remote execution with unconfirmed completion keeps ownership even
   * after lease expiry. Only canonical result reconciliation can release it. */
  native_handoff?: NativeClaimHold;
}

export interface NativeClaimBinding {
  run_id: string;
  node_id: string;
  context_sha256: string;
}

export interface NativeClaimHold extends NativeClaimBinding {
  phase: 'reserved' | 'offered' | 'running' | 'reconciling' | 'result_pending';
  held_at: string;
}

/** Emitted by the native ledger receiver only after its append/replay is
 * durable. Process exit and worker-provided success flags are not this proof. */
export interface NativeResultAcknowledgement extends NativeClaimBinding {
  profile: string;
  work_id: string;
  receipt_sha256: string;
  outcome: 'recorded' | 'already_recorded';
}

export interface ClaimConflictResponse {
  error: 'Conflict';
  message: string;
  held_by: ClaimLease;
}

export interface ClaimRenewRequest {
  node_id: string;
  profile: string;
  work_id: string;
  lease_seconds?: number;
}

export interface ClaimReleaseRequest {
  node_id: string;
  profile: string;
  work_id: string;
}
