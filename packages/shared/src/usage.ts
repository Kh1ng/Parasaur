import { USAGE_METRIC_FIELDS, type UsageMeasurements, type UsageMetricField } from '@git-agent-harness/contracts';

export function emptyUsageMeasurements(): UsageMeasurements {
  return Object.fromEntries(USAGE_METRIC_FIELDS.map(field => [field, { reported_total: null, measured_observations: 0, unmeasured_observations: 0 }])) as UsageMeasurements;
}

/** Observe fields independently. In particular, input/output never manufacture a total. */
export function observeUsage(values: Partial<Record<UsageMetricField, unknown>>): UsageMeasurements {
  const result = emptyUsageMeasurements();
  for (const field of USAGE_METRIC_FIELDS) {
    const value = values[field];
    if (typeof value === 'number' && Number.isFinite(value) && value >= 0 && (field.endsWith('_usd') || Number.isSafeInteger(value))) {
      result[field].reported_total = value;
      result[field].measured_observations = 1;
    } else result[field].unmeasured_observations = 1;
  }
  return result;
}

/** Combine disjoint observations; a source with an unavailable subtotal stays unavailable. */
export function addUsageMeasurements(target: UsageMeasurements, source: UsageMeasurements): void {
  for (const field of USAGE_METRIC_FIELDS) {
    const into = target[field];
    const from = source[field];
    if (from.measured_observations > 0) {
      if (into.measured_observations === 0) into.reported_total = from.reported_total;
      else if (into.reported_total !== null && from.reported_total !== null) {
        const sum = into.reported_total + from.reported_total;
        into.reported_total = Number.isFinite(sum) && (field.endsWith('_usd') || Number.isSafeInteger(sum)) ? sum : null;
      } else into.reported_total = null;
    }
    into.measured_observations += from.measured_observations;
    into.unmeasured_observations += from.unmeasured_observations;
  }
}
