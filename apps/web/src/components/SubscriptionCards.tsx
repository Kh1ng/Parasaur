import { useEffect, useState } from 'react';
import { ChevronRight } from 'lucide-react';
import { formatUntil, projectedPercent, recordedCostLabel, usageTone, type SubscriptionUsage } from '../lib/subscriptionUsage.js';
import { formatAge } from '../lib/format.js';
import { UsageRing } from './UsageRing.js';
import { agentDisplayName } from './LiveAgentsCard.js';
import { UsageWindowRow } from './SubscriptionUsageMenu.js';

const DOT_CLASS = { good: 'bg-good', warning: 'bg-warning', critical: 'bg-critical', unknown: 'bg-muted/40' } as const;

function SubscriptionCard({ usage, now, open, onToggle, unavailable }: { usage: SubscriptionUsage; now: number; open: boolean; onToggle: () => void; unavailable: boolean }) {
  const tight = usage.tightest;
  return (
    <article className="card overflow-hidden" aria-label={`${usage.providerLabel} subscription`}>
      <div className="flex items-center gap-2 px-3 py-2">
        <button type="button" onClick={onToggle} aria-expanded={open} className="flex min-w-0 flex-1 items-center gap-2 text-left">
          <UsageRing usage={usage} size={24} unconfirmed={unavailable} />
          <span className="flex min-w-0 flex-1 items-baseline gap-1.5">
            <span className="truncate text-sm font-medium text-primary">{usage.providerLabel}</span>
            <span className="truncate text-xs text-muted">{agentDisplayName(usage.id)}{usage.model ? ` · ${usage.model}` : ''}</span>
          </span>
          <ChevronRight size={16} className={`shrink-0 text-muted transition-transform ${open ? 'rotate-90' : ''}`} aria-hidden="true" />
        </button>
      </div>
      {!open && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 pb-2 text-xs">
          {usage.windows.length === 0 && <span className="text-muted">{usage.eligible ? 'No usage figures yet' : usage.reason ?? 'Unavailable'}</span>}
          {usage.windows.map((window) => (
            <span key={window.key} className="flex items-center gap-1 whitespace-nowrap">
              <span aria-hidden="true" className={`h-1.5 w-1.5 shrink-0 rounded-full ${DOT_CLASS[usageTone(window.usedPercent)]}`} />
              <span className="text-muted">{window.label}</span>
              <span className="tabular-nums text-primary">{window.usedPercent === null ? '—' : `${Math.round(window.usedPercent)}%`}</span>
            </span>
          ))}
          {!usage.eligible && usage.reason && usage.windows.length > 0 && <span className="text-warning">{usage.reason}</span>}
        </div>
      )}
      {open && (
        <div className="space-y-3 border-t border-subtle px-3 pb-3 pt-2">
          {tight && tight.usedPercent !== null && (
            <p className="text-xs text-secondary">
              <span className="font-medium text-primary">{unavailable && 'Last recorded: '}{Math.round(tight.usedPercent)}% of {tight.label.toLowerCase()} used.</span>
              {!unavailable && formatUntil(tight.resetAt, now) && <> Resets in {formatUntil(tight.resetAt, now)}.</>}
              {!unavailable && projectedPercent(tight, now) !== null && <> At this pace ≈{projectedPercent(tight, now)}% by reset.</>}
            </p>
          )}
          {usage.windows.length === 0 && <p className="text-xs text-muted">{usage.eligible ? 'This provider reports no usage windows.' : usage.reason ?? 'Unavailable'}</p>}
          {usage.windows.map((window) => <UsageWindowRow key={window.key} window={window} now={now} unconfirmed={unavailable} />)}
          <p className="text-[11px] text-muted">
            {usage.observedAt ? `Observed ${formatAge(usage.observedAt) ?? usage.observedAt}` : 'Not observed yet'}
            {!usage.eligible && usage.reason && ` · ${usage.reason}`}
          </p>
          <p className="text-xs tabular-nums text-muted">{recordedCostLabel(usage)}</p>
        </div>
      )}
    </article>
  );
}

/**
 * Every subscription as a collapsible card: collapsed, one line of window
 * chips; open, each window's bar, reset time and pace projection.
 */
export function SubscriptionCards({ subscriptions, unavailable = false }: { subscriptions: SubscriptionUsage[]; unavailable?: boolean }) {
  const [open, setOpen] = useState<Set<string>>(() => new Set());
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, []);
  if (subscriptions.length === 0) return null;
  const toggle = (id: string) => setOpen((current) => {
    const next = new Set(current);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });
  return (
    <section aria-labelledby="subscriptions-title" className="space-y-2">
      <h3 id="subscriptions-title" className="text-base font-semibold text-primary">Subscriptions</h3>
      {unavailable && <p className="text-xs text-warning">Last recorded allowances and costs. Current usage is unconfirmed.</p>}
      <p className="text-xs text-muted">Costs cover recorded executions in this profile; provider billing may differ.</p>
      {subscriptions.map((usage) => <SubscriptionCard key={usage.id} usage={usage} now={now} open={open.has(usage.id)} onToggle={() => toggle(usage.id)} unavailable={unavailable} />)}
    </section>
  );
}
