/** Canonical `gah report --usage --json` output. Local ledger scope only;
 * this is recorded execution telemetry, not a provider subscription bill. */
export interface DispatchUsageReport {
  schema_version: 1;
  profile: string | null;
  since: string;
  generated_at: string;
  entries: number;
  excluded_preflight_entries: number;
  rows: DispatchUsageRow[];
}

export interface DispatchUsageRow {
  profile: string;
  project_provider: string;
  repo: string;
  work_id: string | null;
  backend: string;
  backend_instance: string | null;
  account_label: string | null;
  auth_source_label: string | null;
  quota_pool: string | null;
  requested_model: string | null;
  effective_model: string | null;
  actual_model: string | null;
  model_provider: string | null;
  usage_classification: string | null;
  /** UTC date of the dispatch record, not an invented per-attempt timestamp. */
  day: string;
  dispatch_attempts: number;
  /** Dispatches with no per-attempt usage breakdown. Their known attempt count
   * is still included above; this is not a second set of attempts. */
  legacy_dispatches: number;
  /** Each sum requires a reading from every included attempt/legacy record. */
  input_tokens: number | null;
  output_tokens: number | null;
  total_tokens: number | null;
  cache_read_tokens: number | null;
  cache_write_tokens: number | null;
  reasoning_tokens: number | null;
  actual_cost_usd: number | null;
  estimated_cost_usd: number | null;
  measurements: UsageMeasurements;
}

export const USAGE_METRIC_FIELDS = ['input_tokens', 'output_tokens', 'total_tokens', 'cache_read_tokens', 'cache_write_tokens', 'reasoning_tokens', 'actual_cost_usd', 'estimated_cost_usd'] as const;
export type UsageMetricField = typeof USAGE_METRIC_FIELDS[number];

/** A measured subtotal is not a complete total when readings or a source are missing. */
export interface UsageMeasurement {
  reported_total: number | null;
  measured_observations: number;
  unmeasured_observations: number;
}
export type UsageMeasurements = Record<UsageMetricField, UsageMeasurement>;

export interface RecordedUsageRow extends Omit<DispatchUsageRow, UsageMetricField | 'project_provider' | 'repo'> {
  source: 'chat' | 'dispatch';
  chat_turns: number;
  node_id: string | null;
  /** Historical chat logs do not record repository/forge identity. */
  project_provider: string | null;
  repo: string | null;
}

export interface UsageSourceCoverage {
  status: 'available' | 'unavailable';
  message?: string;
  excluded_preflight_entries?: number;
}

export interface RecordedUsageReport {
  schema_version: 1;
  scope: 'recorded-ledger-and-chat';
  profile: string | null;
  since: string;
  generated_at: string;
  sources: { chat: UsageSourceCoverage; dispatch: UsageSourceCoverage };
  rows: RecordedUsageRow[];
  totals: UsageMeasurements;
}
