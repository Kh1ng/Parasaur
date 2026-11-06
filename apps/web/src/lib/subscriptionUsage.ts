import type { QuotaCandidateStatus, QuotaCheck, QuotaObservation, QuotaSnapshot, RunningWorker, Session } from '@git-agent-harness/contracts';
import { formatCost } from './format.js';

/** One rate-limit window of a subscription, as the provider reports it. */
export interface UsageWindow {
  key: string;
  /** Session (5h), Weekly, Daily, or the provider's own name for it. */
  label: string;
  usedPercent: number | null;
  resetAt: string | null;
  /** Length of the window when its name says so (5h, weekly, 10080m). */
  windowMs: number | null;
  observedAt: string | null;
  /** The model this window is for, when the provider splits by model. */
  model: string | null;
  /** The account's quota pool this window belongs to (Gemini, External models, Vibe…). */
  pool: string | null;
}

/** A subscription: everything the quota snapshot knows about one account. */
export interface SubscriptionUsage {
  /** The account: its named credential, or the backend instance without its pool suffix. */
  id: string;
  backend: string;
  /** Exact node-local execution scopes represented by this account. */
  executionScopes: Array<{ backend: string; backendInstance: string }>;
  /** Subscription provider: openai, anthropic, antigravity, z-ai… */
  provider: string | null;
  providerLabel: string;
  model: string | null;
  windows: UsageWindow[];
  /** The window closest to its limit. */
  tightest: UsageWindow | null;
  eligible: boolean;
  reason: string | null;
  unavailableUntil: string | null;
  /** Recorded ledger costs, scoped to exact execution identities, not provider billing. */
  costUsd: number | null;
  estimatedCostUsd?: number | null;
  costPeriod?: string | null;
  costUnknownObservations?: number;
  observedAt: string | null;
}

const PROVIDER_LABELS: Record<string, string> = {
  openai: 'OpenAI', anthropic: 'Anthropic', antigravity: 'Antigravity', 'z-ai': 'Z.ai', google: 'Google',
  mistral: 'Mistral', nous: 'Nous', github: 'GitHub', 'github-copilot': 'GitHub Copilot'
};

export function providerLabel(provider: string | null | undefined, backend: string): string {
  const key = (provider ?? backend).toLowerCase();
  return PROVIDER_LABELS[key] ?? key.charAt(0).toUpperCase() + key.slice(1);
}

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

/** Turn a provider's window name into a label and a length, when the name carries one. */
export function parseWindow(name: string | null | undefined): { label: string; windowMs: number | null } {
  const raw = (name ?? '').trim();
  const lower = raw.toLowerCase();
  if (!raw) return { label: 'Usage', windowMs: null };
  if (/^(weekly|7 ?d(ays?)?|10080 ?m(in)?)$/.test(lower)) return { label: 'Weekly', windowMs: 7 * DAY };
  if (/^(daily|1 ?d(ay)?|24 ?h(ours?)?|1440 ?m(in)?)$/.test(lower)) return { label: 'Daily', windowMs: DAY };
  if (/(^|[-_ ])monthly$/.test(lower)) return { label: 'Monthly', windowMs: 30 * DAY };
  const hours = /^(\d+)[ -]?h(ours?)?$/.exec(lower);
  if (hours) return { label: `Session (${hours[1]}h)`, windowMs: Number(hours[1]) * HOUR };
  const minutes = /^(\d+) ?m(in(utes?)?)?$/.exec(lower);
  if (minutes) {
    const count = Number(minutes[1]);
    if (count % 1440 === 0) return { label: count === 1440 ? 'Daily' : `${count / 1440}-day`, windowMs: count * 60_000 };
    if (count % 60 === 0) return { label: `Session (${count / 60}h)`, windowMs: count * 60_000 };
    return { label: `${count} min`, windowMs: count * 60_000 };
  }
  return { label: raw, windowMs: null };
}

function usedPercent(observation: QuotaObservation): number | null {
  const remaining = observation.quota_remaining_percent;
  if (typeof remaining === 'number' && Number.isFinite(remaining)) return 100 - Math.min(100, Math.max(0, remaining));
  return null;
}

/** `agy-second:external` -> `agy-second`; a named credential wins. */
export function accountId(source: { backend: string; backend_instance?: string | null; credential_id?: string | null }): string {
  return source.credential_id ?? (source.backend_instance ?? source.backend).split(':')[0];
}

const POOL_LABELS: Record<string, string> = {
  external: 'External models', 'google-native': 'Gemini', gemini: 'Gemini',
  vibe: 'Vibe', 'vibe-monthly': 'Vibe', 'vibe-code-included-monthly': 'Vibe', api: 'API', admin: 'API'
};

/** `agy-second:external` -> `External models`; null when the scope names no pool. */
function poolLabel(source: { backend_instance?: string | null; quota_pool?: string | null }, account: string): string | null {
  const raw = [source.quota_pool, source.backend_instance]
    .map((value) => value?.split(':').slice(1).join(':') || (value && value !== account ? value : null))
    .find(Boolean);
  if (!raw) return null;
  return POOL_LABELS[raw.toLowerCase()] ?? raw.replace(/[-_]+/g, ' ').replace(/^\w/, (c) => c.toUpperCase());
}

function toWindow(observation: QuotaObservation, index: number, pool: string | null): UsageWindow {
  const { label, windowMs } = parseWindow(observation.quota_window);
  const base = observation.model && observation.model !== label ? `${label} · ${observation.model}` : label;
  return {
    key: `${pool ?? ''}-${observation.quota_window ?? 'usage'}-${observation.model ?? ''}-${index}`,
    label: pool ? `${pool} · ${base}` : base,
    usedPercent: usedPercent(observation),
    resetAt: observation.quota_reset_at ?? null,
    windowMs,
    observedAt: observation.observed_at ?? null,
    model: observation.model ?? null,
    pool
  };
}

type Scope = Pick<QuotaCheck, 'backend' | 'provider' | 'backend_instance' | 'credential_id' | 'quota_pool' | 'quota_observations'>;

/**
 * One entry per account, not per routing candidate or model (#1411): an
 * account's pools (Antigravity's Gemini and external-model pools, Mistral's
 * API and Vibe pools) are windows of the same ring. Router aliases
 * (`cli-router…`) route onto those accounts and are not accounts themselves.
 */
export function subscriptionUsage(snapshot: Partial<Pick<QuotaSnapshot, 'candidates' | 'quota_checks' | 'account_spend' | 'since'>> | null | undefined): SubscriptionUsage[] {
  const candidates = snapshot?.candidates ?? [];
  const scopes: Scope[] = [...candidates, ...(snapshot?.quota_checks ?? [])];
  const accounts = new Map<string, { scopes: Scope[]; candidates: QuotaCandidateStatus[] }>();
  for (const scope of scopes) {
    const id = accountId(scope);
    if (id.startsWith('cli-router')) continue;
    const entry = accounts.get(id) ?? { scopes: [], candidates: [] };
    entry.scopes.push(scope);
    if (candidates.includes(scope as QuotaCandidateStatus)) entry.candidates.push(scope as QuotaCandidateStatus);
    accounts.set(id, entry);
  }
  return [...accounts.entries()].map(([id, account]) => {
    const pools = new Set(account.scopes.map((scope) => poolLabel(scope, id)).filter(Boolean));
    const byWindow = new Map<string, UsageWindow>();
    account.scopes.forEach((scope) => (scope.quota_observations ?? []).forEach((observation, index) => {
      const window = toWindow(observation, index, pools.size > 1 ? poolLabel(scope, id) : null);
      const key = `${window.pool}|${observation.quota_window}|${window.model}`;
      const previous = byWindow.get(key);
      // A candidate and its account check report the same reading; keep the newest.
      if (!previous || (window.observedAt ?? '') > (previous.observedAt ?? '')) byWindow.set(key, window);
    }));
    // An account-wide reading repeated under each pool is shown once, per pool.
    const pooled = new Set([...byWindow.values()].filter((window) => window.pool).map((window) => window.windowMs ?? window.label));
    const windows = [...byWindow.values()].filter((window) => window.pool || !pooled.has(window.windowMs ?? window.label))
      // Short windows first, then by name, so the ring's outer arc is the session.
      .sort((a, b) => (a.windowMs ?? Infinity) - (b.windowMs ?? Infinity) || a.label.localeCompare(b.label));
    const measured = windows.filter((window) => window.usedPercent !== null);
    const tightest = measured.length ? measured.reduce((max, window) => (window.usedPercent! > max.usedPercent! ? window : max)) : null;
    const routed = account.candidates;
    const blocked = routed.length > 0 && routed.every((candidate) => !candidate.eligible_now) ? routed[0] : null;
    const first = account.scopes[0];
    const provider = account.scopes.map((scope) => scope.provider).find(Boolean) ?? null;
    // Candidate totals may overlap across models/pools and include other accounts.
    // Only the producer's canonical, exact-instance rows can supply account costs.
    const spend = (snapshot?.account_spend ?? []).filter(row => row.backend_instance !== null
      && account.scopes.some(scope => scope.backend === row.backend && scope.backend_instance === row.backend_instance));
    const total = (field: 'actual_cost_usd' | 'estimated_cost_usd') => {
      const costs = spend.map(row => row[field]).filter((cost): cost is number => typeof cost === 'number' && Number.isFinite(cost));
      return costs.length ? costs.reduce((sum, cost) => sum + cost, 0) : null;
    };
    return {
      id,
      backend: first.backend,
      executionScopes: account.scopes.flatMap(scope => scope.backend_instance ? [{ backend: scope.backend, backendInstance: scope.backend_instance }] : []),
      provider,
      providerLabel: providerLabel(provider, first.backend),
      model: null,
      windows,
      tightest,
      eligible: !blocked,
      reason: blocked?.reason ?? null,
      unavailableUntil: blocked?.unavailable_until ?? null,
      costUsd: total('actual_cost_usd'),
      estimatedCostUsd: total('estimated_cost_usd'),
      costPeriod: snapshot?.since ?? null,
      costUnknownObservations: spend.reduce((sum, row) => sum + row.actual_cost_unknown_observations, 0),
      observedAt: windows.map((window) => window.observedAt).filter((value): value is string => !!value).sort().at(-1)
        ?? routed.map((candidate) => candidate.observed_at).filter((value): value is string => !!value).sort().at(-1) ?? null
    };
  }).sort((a, b) => a.providerLabel.localeCompare(b.providerLabel) || a.id.localeCompare(b.id));
}

/** The same qualified ledger-cost label in the navbar and account cards. */
export function recordedCostLabel(usage: SubscriptionUsage): string {
  const period = usage.costPeriod ? ` (${usage.costPeriod})` : '';
  const actual = `Recorded actual cost${period}: ${formatCost(usage.costUsd)}`;
  const estimate = usage.estimatedCostUsd != null ? ` · Estimated cost: ${formatCost(usage.estimatedCostUsd)}` : '';
  return actual + estimate + (usage.costUnknownObservations ? ' · Some recorded costs are unknown.' : '');
}

/** Confirmed account activity from live sessions and fresh dispatch-runtime
 * observations. Claims, controller intent, process names and transport IDs do
 * not identify an executing account. Tagged observations belong to their node.
 */
export function busySubscriptionIds(input: {
  subscriptions: SubscriptionUsage[];
  sessions: Session[];
  runningWorkers?: RunningWorker[];
  nodeId?: string | null;
  now?: number;
}): Set<string> {
  const now = input.now ?? Date.now();
  const local = (nodeId: string | null | undefined) => !nodeId || nodeId === input.nodeId;
  const account = (backend: string | undefined, instance: string | undefined) => {
    if (!backend || !instance) return undefined;
    const matches = input.subscriptions.filter(usage => usage.executionScopes.some(scope =>
      scope.backend === backend && scope.backendInstance === instance));
    return matches.length === 1 ? matches[0] : undefined;
  };
  const busy = new Set<string>();
  for (const session of input.sessions) {
    if (session.status !== 'running' || !local(session.nodeId)) continue;
    const match = account(session.backend, session.backendInstance);
    if (match) busy.add(match.id);
  }
  for (const worker of input.runningWorkers ?? []) {
    const age = now - Date.parse(worker.last_activity_at);
    if (worker.state !== 'running' || !local(worker.node_id) || !Number.isFinite(age)
      || age < 0 || !Number.isFinite(worker.stale_after_seconds) || worker.stale_after_seconds <= 0
      || age > worker.stale_after_seconds * 1000) continue;
    const match = account(worker.backend, worker.backend_instance);
    if (match) busy.add(match.id);
  }
  return busy;
}

/** A passed or invalid reset cannot confirm the current allowance. No reset
 * is allowed for providers that report usage without a scheduled window. */
export function usageWindowIsCurrent(window: UsageWindow, now: number): boolean {
  return window.resetAt === null || Date.parse(window.resetAt) > now;
}

/** Historical readings remain available, but cannot select the current limit. */
export function currentTightestWindow(usage: SubscriptionUsage, now: number): UsageWindow | null {
  return usage.windows.filter(window => window.usedPercent !== null && usageWindowIsCurrent(window, now))
    .reduce<UsageWindow | null>((max, window) => !max || window.usedPercent! > max.usedPercent! ? window : max, null);
}

/** Where the window will be at reset if use keeps its pace so far: used%
 * scaled from the elapsed part of the window to all of it. Null until a
 * meaningful slice of the window has passed. */
export function projectedPercent(window: UsageWindow, now: number): number | null {
  if (window.usedPercent === null || !window.windowMs || !window.resetAt) return null;
  const remaining = Date.parse(window.resetAt) - now;
  if (!Number.isFinite(remaining) || remaining <= 0 || remaining > window.windowMs) return null;
  const elapsed = window.windowMs - remaining;
  // The first tenth of a window says little about the pace; a fresh session would read 100%.
  if (elapsed < window.windowMs * 0.1) return null;
  return Math.min(100, Math.round((window.usedPercent * window.windowMs) / elapsed));
}

/** Time until a future reset. Past resets do not predict the next window. */
export function formatUntil(iso: string | null, now: number): string | null {
  if (!iso) return null;
  const ms = Date.parse(iso) - now;
  if (!Number.isFinite(ms)) return null;
  if (ms <= 0) return null;
  const minutes = Math.round(ms / 60_000);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ${String(minutes % 60).padStart(2, '0')}m`;
  const days = Math.floor(hours / 24);
  return `${days}d ${hours % 24}h`;
}

/** Tone for a used percentage: fine, getting close, or nearly out. */
export function usageTone(percent: number | null): 'good' | 'warning' | 'critical' | 'unknown' {
  if (percent === null) return 'unknown';
  if (percent >= 90) return 'critical';
  if (percent >= 70) return 'warning';
  return 'good';
}
