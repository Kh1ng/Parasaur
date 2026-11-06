import type { UsageRollupRow } from '@git-agent-harness/contracts';
import { summarizeChatUsage } from './chatUsage.js';

/** Roll up provider readings without deriving totals or cache counters from other fields. */
export function aggregateUsageRows(rows: UsageRollupRow[]) {
  const groups = new Map<string, UsageRollupRow[]>();
  for (const row of rows) {
    const key = JSON.stringify([row.backend, row.backend_instance]);
    const group = groups.get(key) ?? [];
    group.push(row);
    groups.set(key, group);
  }
  const summarize = (group: UsageRollupRow[]) => ({
    ...summarizeChatUsage(group.map(row => ({ ...row, duration_seconds: null }))),
    turns: group.reduce((sum, row) => sum + row.turns, 0)
  });
  return [...groups.entries()].map(([id, group]) => {
    const row = group[0]!;
    const models = new Map<string | null, UsageRollupRow[]>();
    for (const entry of group) {
      const modelRows = models.get(entry.model) ?? [];
      modelRows.push(entry);
      models.set(entry.model, modelRows);
    }
    return [row.backend_instance ? `${row.backend} · ${row.backend_instance}` : row.backend, {
      ...summarize(group), id,
      models: new Map([...models.entries()].map(([model, entries]) => [model, summarize(entries)]))
    }] as const;
  }).sort((a, b) => (b[1].totalTokens ?? -1) - (a[1].totalTokens ?? -1));
}
