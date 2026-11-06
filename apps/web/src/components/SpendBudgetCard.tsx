import type { StatusSnapshot } from '@git-agent-harness/contracts';
import { formatCost, formatCount, formatLocalTime } from '../lib/format.js';
import { StatusBadge } from './ui/StatusBadge.js';

/** Display the node's admission decision without inferring missing costs. */
export function SpendBudgetCard({ budget }: { budget: StatusSnapshot['spend_budget'] }) {
  if (!budget) return null;
  return <section className="card-padded" aria-label="Factory spend ceilings">
    <div className="flex flex-wrap items-center justify-between gap-2">
      <h3 className="text-sm font-semibold text-primary">Factory spend ceilings</h3>
      <StatusBadge label={budget.intake_held ? 'Intake paused' : 'Within recorded ceiling'} tone={budget.intake_held ? 'warning' : 'good'} />
    </div>
    <p className="mt-1 text-xs text-muted">Based on {budget.cost_basis === 'actual' ? 'reported actual cost' : 'API-equivalent estimates'} in this node’s factory ledger.</p>
    {budget.hold_reason && <p role="status" className="mt-3 text-sm text-warning break-words">{budget.hold_reason}</p>}
    <dl className="mt-3 grid gap-4 sm:grid-cols-2">
      {budget.windows.map(window => <div key={window.window} className="min-w-0">
        <dt className="text-xs text-muted">{window.window === 'day' ? 'Daily' : 'Monthly'} · UTC</dt>
        <dd className="mt-1 text-sm tabular-nums text-primary">
          {window.reported_usd === null ? 'Cost not reported' : `${formatCost(window.reported_usd)} recorded`} / {formatCost(window.limit_usd)} limit
        </dd>
        {window.incomplete && <dd className="mt-1 text-xs text-warning">{window.unmeasured_observations > 0 ? `${formatCount(window.unmeasured_observations)} cost readings missing` : 'Cost coverage incomplete'}</dd>}
        <dd className="mt-1 text-xs text-muted">Resets {formatLocalTime(window.resets_at) ?? 'at an unknown time'}</dd>
      </div>)}
    </dl>
    <p className="mt-3 text-xs text-muted">Running work can finish above a ceiling. Chat and other nodes’ history are outside this limit.</p>
  </section>;
}
