import type { RecordedUsageRow } from '@git-agent-harness/contracts';
import { addUsageMeasurements, emptyUsageMeasurements } from '@git-agent-harness/shared';

export const USAGE_GROUPINGS = ['backend', 'model', 'project', 'work-item', 'day'] as const;
export type UsageGrouping = typeof USAGE_GROUPINGS[number];

/** Group presentation axes without collapsing account/node or requested/actual model identity. */
export function aggregateUsageRows(rows: RecordedUsageRow[], grouping: UsageGrouping = 'backend') {
  const groups = new Map<string, { key: string; label: string; detail: string; chat_turns: number;
    dispatch_attempts: number; legacy_dispatches: number; measurements: ReturnType<typeof emptyUsageMeasurements> }>();
  for (const row of rows) {
    const account = [row.backend_instance ?? 'Account not recorded', row.account_label,
      row.auth_source_label ? `Auth: ${row.auth_source_label}` : null, row.quota_pool ? `Pool: ${row.quota_pool}` : null,
      row.node_id ?? 'Execution node not recorded'].filter(Boolean).join(' · ');
    const project = `${row.profile} · ${row.project_provider ?? 'Forge not recorded'} · ${row.repo ?? 'Repository not recorded'}`;
    const identity = grouping === 'backend' ? [row.backend, row.backend_instance, row.node_id, row.account_label, row.auth_source_label, row.quota_pool]
      : grouping === 'model' ? [row.backend, row.backend_instance, row.node_id, row.account_label, row.auth_source_label, row.quota_pool, row.requested_model, row.effective_model, row.actual_model]
        : grouping === 'project' ? [row.profile, row.project_provider, row.repo]
          : grouping === 'work-item' ? [row.profile, row.project_provider, row.repo, row.work_id] : [row.day];
    const key = JSON.stringify(identity);
    const label = grouping === 'backend' ? row.backend : grouping === 'model' ? (row.actual_model ?? row.effective_model ?? 'Model not recorded')
      : grouping === 'project' ? row.profile : grouping === 'work-item' ? (row.work_id ?? 'Work item not recorded') : row.day;
    const detail = grouping === 'backend' ? account : grouping === 'model'
      ? `${row.backend} · ${account} · Requested: ${row.requested_model ?? 'unknown'} · Route: ${row.effective_model ?? 'unknown'} · Reported: ${row.actual_model ?? 'unknown'}`
      : grouping === 'project' ? `${row.project_provider ?? 'Forge not recorded'} · ${row.repo ?? 'Repository not recorded'}`
        : grouping === 'work-item' ? project : 'UTC record date';
    const group = groups.get(key) ?? { key, label, detail, chat_turns: 0, dispatch_attempts: 0, legacy_dispatches: 0, measurements: emptyUsageMeasurements() };
    group.chat_turns += row.chat_turns;
    group.dispatch_attempts += row.dispatch_attempts;
    group.legacy_dispatches += row.legacy_dispatches;
    addUsageMeasurements(group.measurements, row.measurements);
    groups.set(key, group);
  }
  return [...groups.values()].sort((a, b) => grouping === 'day' ? b.label.localeCompare(a.label) : a.label.localeCompare(b.label) || a.key.localeCompare(b.key));
}
