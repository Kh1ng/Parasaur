/** Owner-visible node output. Missing measurements are unknown, never zero. */
export type ComputeOutputEntry = {
  id: string; revision: string | null; workId: string | null;
  archiveBytes: number | null; incomplete: boolean; discardable: boolean;
  state: 'retained' | 'running' | 'unresolved';
};
export type ComputeOutputCatalog = {
  maxJobs: number; maxArchiveBytes: number; entries: ComputeOutputEntry[];
  /** Unknown root contents block admission and require host-side inspection. */
  admissionBlocked: boolean;
};
/** Hashes describe observed download bytes, not producer attestations. */
export type ComputeOutputDownload = {
  bytes: number; blockBytes: number; sha256: string; blocks: string[];
  filename: string; incomplete: boolean;
};
