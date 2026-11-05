/** Owner-visible node output. Missing measurements are unknown, never zero. */
export type ComputeOutputEntry = {
  id: string; revision: string | null; workId: string | null;
  archiveBytes: number | null; incomplete: boolean; discardable: boolean;
  state: 'retained' | 'running' | 'unresolved';
  /** Missing on old nodes or when the receipt cannot be read. Retirement does
   * not confirm that the local result was delivered or replace its exit code. */
  delivery?: 'pending' | 'delivered' | 'retired';
};
export type ComputeOutputCatalog = {
  maxJobs: number; maxArchiveBytes: number; entries: ComputeOutputEntry[];
  /** Unknown root contents block admission and require host-side inspection. */
  admissionBlocked: boolean;
  /** Absent on older nodes. Null readings remain unknown. */
  journal?: ComputeJournalStatus;
};
export type ComputeJournalStatus = { entries: number | null; limit: number | null; cleanupAvailable: boolean };
export type ComputeJournalCandidate = { id: string; revision: string; workId: string; delivery?: 'delivered' | 'retired' };
export type ComputeJournalReview = {
  entries: number; limit: number; storageVerified: boolean; revision: string;
  candidates: ComputeJournalCandidate[];
};
export type ComputeJournalCleanupReason = 'cloud_unavailable' | 'claim_not_retired' | 'storage_changed' | 'receipt_changed';
export type ComputeJournalCleanup = { removed: number; retained: number; unconfirmed: number; interrupted: boolean;
  retainedReasons?: ComputeJournalCleanupReason[] };
/** Hashes describe observed download bytes, not producer attestations. */
export type ComputeOutputDownload = {
  bytes: number; blockBytes: number; sha256: string; blocks: string[];
  filename: string; incomplete: boolean;
};
