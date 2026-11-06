import { useEffect, useMemo, useState } from 'react';
import { Download, FlaskConical, RefreshCw } from 'lucide-react';
import type { RecordedUsageReport, UsageMeasurement } from '@git-agent-harness/contracts';
import { gahApi } from '../api/client.js';
import { useAutoRefresh } from '../hooks/useAutoRefresh.js';
import { useWsReconnectRefresh } from '../hooks/useWsReconnectRefresh.js';
import { aggregateUsageRows, USAGE_GROUPINGS, type UsageGrouping } from '../lib/usageRollup.js';
import { formatCost, formatCount, formatTokens } from '../lib/format.js';
import { EmptyState, LoadingState } from './ui/EmptyState.js';
import { StatusBadge } from './ui/StatusBadge.js';

function Reading({ reading, cost = false }: { reading: UsageMeasurement; cost?: boolean }) {
  return <span className="block tabular-nums">
    <span className={reading.reported_total === null ? 'text-muted' : 'text-primary'}>
      {reading.reported_total === null ? 'Not reported' : cost ? formatCost(reading.reported_total) : formatTokens(reading.reported_total)}
    </span>
    {reading.unmeasured_observations > 0 && <span className="mt-0.5 block text-xs text-warning">{formatCount(reading.unmeasured_observations)} missing</span>}
  </span>;
}

export function RecordedUsageCard({ profile }: { profile: string | undefined }) {
  const [days, setDays] = useState<7 | 30>(7);
  const [grouping, setGrouping] = useState<UsageGrouping>('backend');
  const [data, setData] = useState<{ key: string; report: RecordedUsageReport } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [retry, setRetry] = useState(0);
  const key = JSON.stringify([profile ?? null, days]);
  const report = data?.key === key ? data.report : null;
  const refresh = () => setRetry(value => value + 1);
  useAutoRefresh(refresh, 60_000);
  useWsReconnectRefresh(refresh);
  useEffect(() => {
    let cancelled = false;
    setLoading(true); setError(null);
    gahApi.getRecordedUsage(profile, days).then(report => { if (!cancelled) setData({ key, report }); })
      .catch(cause => { if (!cancelled) { setError(cause instanceof Error ? cause.message : String(cause)); setData(null); } })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [key, retry]);
  const groups = useMemo(() => aggregateUsageRows(report?.rows ?? [], grouping), [report, grouping]);
  const counts = (report?.rows ?? []).reduce((sum, row) => ({ chat: sum.chat + row.chat_turns,
    attempts: sum.attempts + row.dispatch_attempts, summaries: sum.summaries + row.legacy_dispatches }), { chat: 0, attempts: 0, summaries: 0 });
  const complete = report && Object.values(report.sources).every(source => source.status === 'available');
  const exportReport = () => {
    if (!report) return;
    const url = URL.createObjectURL(new Blob([JSON.stringify(report, null, 2) + '\n'], { type: 'application/json' }));
    const link = document.createElement('a'); link.href = url; link.download = `parasaur-usage-${days}d.json`; link.click();
    setTimeout(() => URL.revokeObjectURL(url), 0);
  };
  return <section className="card-padded" aria-labelledby="recorded-usage-title" aria-busy={loading}>
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div><h3 id="recorded-usage-title" className="text-sm font-semibold text-primary">Recorded usage</h3>
        <p className="mt-1 max-w-2xl text-xs text-muted">Chat and factory activity recorded here. Other nodes may have additional history. Provider quotas and bills are separate.</p></div>
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex overflow-hidden rounded-md border border-subtle text-xs">{([7, 30] as const).map(value => <button key={value}
          aria-pressed={days === value} onClick={() => setDays(value)} className={`px-3 py-1.5 ${days === value ? 'bg-accent-fill text-on-fill' : 'text-secondary hover:bg-overlay/5'}`}>{value}d</button>)}</div>
        <button type="button" className="btn-secondary text-xs" onClick={refresh} disabled={loading} aria-label="Refresh recorded usage"><RefreshCw className="h-3.5 w-3.5" /></button>
        <button type="button" className="btn-secondary text-xs" onClick={exportReport} disabled={!report}><Download className="h-3.5 w-3.5" />Export JSON</button>
      </div>
    </div>
    {loading && !report ? <LoadingState label="Reading chat and factory usage…" /> : error ? <div role="alert" className="mt-4 text-sm text-critical"><p>{error}</p><button className="btn-secondary mt-2" onClick={refresh}>Retry</button></div> : report && <>
      <div className="mt-4 flex flex-wrap items-center gap-2 text-xs">
        {(['chat', 'dispatch'] as const).map(source => <StatusBadge key={source} tone={report.sources[source].status === 'available' ? 'good' : 'warning'}
          label={`${source === 'chat' ? 'Chat' : 'Factory'} history ${report.sources[source].status}`} />)}
        <span className="text-muted">{profile ?? 'All projects'} · {days}d · UTC record dates · <time dateTime={report.generated_at} title={report.generated_at}>As of {new Date(report.generated_at).toLocaleTimeString()}</time></span>
      </div>
      {Object.entries(report.sources).filter(([, source]) => source.status === 'unavailable').map(([name, source]) => <p key={name} role="alert" className="mt-2 text-xs text-warning">{source.message}</p>)}
      {report.rows.length === 0 ? complete ? <EmptyState icon={FlaskConical} title="No activity recorded" description={`No chat turns or factory usage were recorded in this ${days}-day window.`} />
        : <p className="mt-4 text-sm text-muted">Usage is unavailable. Retry after the source is restored.</p> : <>
        <p className="mt-4 text-xs text-secondary">
          {report.sources.chat.status === 'available' ? `${formatCount(counts.chat)} chat ${counts.chat === 1 ? 'turn' : 'turns'}` : 'Chat count unavailable'} ·{' '}
          {report.sources.dispatch.status === 'available' ? `${formatCount(counts.attempts)} recorded dispatch ${counts.attempts === 1 ? 'attempt' : 'attempts'} · ${formatCount(counts.summaries)} legacy ${counts.summaries === 1 ? 'summary' : 'summaries'}` : 'Factory counts unavailable'}
        </p>
        <p className="mt-1 text-xs text-muted">Amounts below are reported subtotals. Missing readings or sources make the full total unknown. Legacy summaries may describe the same recorded attempts.</p>
        <dl className="mt-4 grid grid-cols-1 gap-4 border-y border-subtle py-3 sm:grid-cols-3">
          <div><dt className="text-xs text-muted">Reported token subtotal</dt><dd className="mt-1 text-sm font-medium"><Reading reading={report.totals.total_tokens} /></dd></div>
          <div><dt className="text-xs text-muted">Recorded cost subtotal</dt><dd className="mt-1 text-sm font-medium"><Reading reading={report.totals.actual_cost_usd} cost /></dd></div>
          <div><dt className="text-xs text-muted">API-equivalent subtotal</dt><dd className="mt-1 text-sm font-medium"><Reading reading={report.totals.estimated_cost_usd} cost /></dd></div>
        </dl>
        <div className="my-4 flex flex-wrap items-center justify-between gap-3">
          <label className="flex items-center gap-2 text-xs text-secondary">Group usage by<select aria-label="Group usage by" value={grouping} onChange={event => setGrouping(event.target.value as UsageGrouping)} className="rounded-md border border-subtle bg-raised px-2 py-1.5 text-primary">
            {USAGE_GROUPINGS.map(value => <option key={value} value={value}>{value === 'day' ? 'UTC day' : value.replace('-', ' ')}</option>)}
          </select></label>
          <span className="text-xs text-muted">Cache and reasoning counts are independent provider readings.</span>
        </div>
        <div className="overflow-x-auto"><table className="w-full text-left text-xs tabular-nums">
          <thead><tr className="border-b border-subtle text-muted">{['Group', 'Chat turns', 'Dispatch attempts', 'Legacy summaries', 'Tokens reported', 'Recorded cost', 'API equivalent'].map(label => <th key={label} className="whitespace-nowrap py-2 pr-4 font-medium">{label}</th>)}</tr></thead>
          <tbody>{groups.map(group => <tr key={group.key} className="border-b border-subtle/50 align-top">
            <td className="min-w-48 max-w-80 py-3 pr-4"><span className="block break-words font-medium text-primary">{group.label}</span><span className="mt-1 block break-words text-muted">{group.detail}</span>
              <details className="mt-2"><summary className="cursor-pointer text-secondary">Token categories</summary><dl className="mt-2 space-y-2">
                {(['input_tokens', 'output_tokens', 'cache_read_tokens', 'cache_write_tokens', 'reasoning_tokens'] as const).map(field => <div key={field} className="flex justify-between gap-3"><dt className="text-muted">{field.replaceAll('_', ' ')}</dt><dd><Reading reading={group.measurements[field]} /></dd></div>)}
              </dl></details></td>
            <td className="py-3 pr-4">{report.sources.chat.status === 'available' ? formatCount(group.chat_turns) : 'Unavailable'}</td><td className="py-3 pr-4">{report.sources.dispatch.status === 'available' ? formatCount(group.dispatch_attempts) : 'Unavailable'}</td><td className="py-3 pr-4">{report.sources.dispatch.status === 'available' ? formatCount(group.legacy_dispatches) : 'Unavailable'}</td>
            <td className="py-3 pr-4"><Reading reading={group.measurements.total_tokens} /></td><td className="py-3 pr-4"><Reading reading={group.measurements.actual_cost_usd} cost /></td><td className="py-3"><Reading reading={group.measurements.estimated_cost_usd} cost /></td>
          </tr>)}</tbody></table></div>
      </>}
    </>}
  </section>;
}
