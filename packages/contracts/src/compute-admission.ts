/** Last reported local compute admission check, not a guarantee that queued
 * work is eligible or that a program is running. Cloud observations expire. */
export type ComputeAdmissionState = 'unknown' | 'disabled' | 'checking' | 'ready' | 'capacity'
  | 'journal_full' | 'storage_full' | 'unavailable' | 'stopped';
