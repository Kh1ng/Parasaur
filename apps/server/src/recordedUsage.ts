import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { readdir } from 'node:fs/promises';
import { join } from 'node:path';
import { USAGE_METRIC_FIELDS, type DispatchUsageReport, type RecordedUsageReport, type RecordedUsageRow, type UsageMeasurements } from '@git-agent-harness/contracts';
import { addUsageMeasurements, emptyUsageMeasurements, observeUsage } from '@git-agent-harness/shared';
import { runDispatchUsageReport } from './gahCli.js';
import { listSessions, stateBase } from './managerChat/chatSessions.js';
import { chatLogPath } from './managerChat/sessionLog.js';

type Options = { profile?: string; days: number };
type Dependencies = { readDispatch?: typeof runDispatchUsageReport; stateDir?: string; now?: () => number };
const DAY = 86_400_000;
const integer = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
const text = (value: unknown): value is string => typeof value === 'string' && value.length > 0;
const nullableText = (value: unknown): value is string | null => value === null || text(value);

/** A bad/older CLI response is a missing source, never an empty successful ledger. */
function dispatchRows(report: DispatchUsageReport, profile?: string): RecordedUsageRow[] {
  if (report.schema_version !== 1 || report.profile !== (profile ?? null) || !Array.isArray(report.rows)
    || !integer(report.entries) || !integer(report.excluded_preflight_entries)) throw Error('Invalid usage source');
  const seen = new Set<string>();
  const rows: RecordedUsageRow[] = report.rows.map(row => {
    if (!text(row.profile) || (profile !== undefined && row.profile !== profile) || !text(row.project_provider)
      || !text(row.repo) || !text(row.backend) || !nullableText(row.work_id)
      || !/^\d{4}-\d{2}-\d{2}$/.test(row.day) || !integer(row.dispatch_attempts) || !integer(row.legacy_dispatches)
      || row.dispatch_attempts + row.legacy_dispatches === 0) throw Error('Invalid usage row');
    for (const field of ['backend_instance', 'account_label', 'auth_source_label', 'quota_pool', 'requested_model', 'effective_model', 'actual_model', 'model_provider', 'usage_classification'] as const) {
      if (!nullableText(row[field])) throw Error('Invalid usage identity');
    }
    const identity = JSON.stringify([row.profile, row.project_provider, row.repo, row.work_id, row.backend, row.backend_instance, row.account_label, row.auth_source_label, row.quota_pool,
      row.requested_model, row.effective_model, row.actual_model, row.model_provider, row.usage_classification, row.day]);
    if (seen.has(identity) || !Number.isFinite(Date.parse(row.day)) || new Date(row.day).toISOString().slice(0, 10) !== row.day
      || row.day < new Date(report.since).toISOString().slice(0, 10)
      || row.day > new Date(report.generated_at).toISOString().slice(0, 10)) throw Error('Duplicate or out-of-window usage group');
    seen.add(identity);
    const measurements = emptyUsageMeasurements();
    let observations: number | undefined;
    for (const field of USAGE_METRIC_FIELDS) {
      const reading = row.measurements?.[field];
      if (!reading || !integer(reading.measured_observations) || !integer(reading.unmeasured_observations)) throw Error('Missing usage coverage');
      const count = reading.measured_observations + reading.unmeasured_observations;
      if (!integer(count) || count === 0 || (observations !== undefined && observations !== count)) throw Error('Invalid usage coverage');
      observations = count;
      if (reading.reported_total !== null && (typeof reading.reported_total !== 'number' || !Number.isFinite(reading.reported_total)
        || reading.reported_total < 0 || (!field.endsWith('_usd') && !Number.isSafeInteger(reading.reported_total)))) throw Error('Invalid usage reading');
      if (reading.measured_observations === 0 && reading.reported_total !== null) throw Error('Unmeasured usage cannot have a subtotal');
      measurements[field] = { ...reading };
    }
    if (observations! > row.dispatch_attempts + row.legacy_dispatches) throw Error('Invalid usage observation count');
    return { source: 'dispatch', chat_turns: 0, node_id: null,
      profile: row.profile, project_provider: row.project_provider, repo: row.repo, work_id: row.work_id,
      backend: row.backend, backend_instance: row.backend_instance, requested_model: row.requested_model,
      account_label: row.account_label, auth_source_label: row.auth_source_label, quota_pool: row.quota_pool,
      effective_model: row.effective_model, actual_model: row.actual_model, model_provider: row.model_provider,
      usage_classification: row.usage_classification, day: row.day, dispatch_attempts: row.dispatch_attempts,
      legacy_dispatches: row.legacy_dispatches, measurements };
  });
  if ((rows.length === 0) !== (report.entries === 0)) throw Error('Inconsistent usage source count');
  return rows;
}

async function directories(path: string, prefix: string): Promise<string[]> {
  try { return (await readdir(path, { withFileTypes: true })).filter(entry => entry.isDirectory() && entry.name.startsWith(prefix)).map(entry => entry.name); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return []; throw error; }
}

/** Stream errors and incomplete JSON reach source coverage; no transcript-sized allocation. */
async function* usageLines(path: string): AsyncGenerator<string> {
  const stream = createReadStream(path, { encoding: 'utf8' });
  const maximum = 16 * 1024 * 1024;
  let pending = '';
  try {
    for await (const chunk of stream) {
      pending += chunk;
      let end: number;
      while ((end = pending.indexOf('\n')) >= 0) {
        if (end > maximum) throw Error('Chat event exceeds the usage scan limit');
        const line = pending.slice(0, end);
        pending = pending.slice(end + 1);
        yield line;
      }
      if (pending.length > maximum) throw Error('Chat event exceeds the usage scan limit');
    }
    if (pending.trim()) yield pending;
  } catch (error) {
    // A session can own saved patches before any conversation was logged.
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
  } finally { stream.destroy(); }
}

/** Scan completed chat readings without retaining prompts, transcripts or whole log files. */
async function chatRows(profile: string | undefined, since: number, until: number, stateDir?: string): Promise<RecordedUsageRow[]> {
  const base = stateDir ?? stateBase();
  const profiles = profile === undefined ? (await directories(base, 'project-')).map(name => {
    const decoded = decodeURIComponent(name.slice(8));
    if (name !== `project-${encodeURIComponent(decoded)}`) throw Error('Noncanonical chat project identity');
    return decoded;
  }) : [profile];
  const groups = new Map<string, RecordedUsageRow>();
  for (const current of profiles) {
    const sessions = new Map(listSessions(current, { stateDir: base }).map(session => [session.id, session]));
    for (const name of await directories(join(base, `project-${encodeURIComponent(current)}`), 'session-')) {
      const id = decodeURIComponent(name.slice(8));
      if (name !== `session-${encodeURIComponent(id)}`) throw Error('Noncanonical chat session identity');
      const session = sessions.get(id);
      const issue = session?.issueNumber;
      const pr = session?.prNumber;
      const work = integer(issue) && issue > 0 ? `#${issue}` : integer(pr) && pr > 0 ? `pr:${pr}` : null;
      const seen = new Map<number, string>();
      for await (const line of usageLines(chatLogPath(current, { stateDir: base, sessionId: id }))) {
        if (!line.trim()) continue;
        const event: unknown = JSON.parse(line);
        if (!event || typeof event !== 'object' || !('type' in event) || typeof event.type !== 'string') throw Error('Invalid chat log');
        if (event.type !== 'assistant/message') continue;
        const value = event as Record<string, unknown>;
        if (!integer(value.seq) || !integer(value.turn) || typeof value.timestamp !== 'number' || !Number.isFinite(value.timestamp)
          || !text(value.backend) || !nullableText(value.model ?? null)) throw Error('Invalid chat reading');
        if (value.timestamp < since || value.timestamp > until) continue;
        const fingerprint = createHash('sha256').update(line).digest('hex');
        if (seen.has(value.seq)) {
          if (seen.get(value.seq) !== fingerprint) throw Error('Conflicting chat event identity');
          continue;
        }
        seen.set(value.seq, fingerprint);
        const backendInstance = value.backendInstance == null ? null : value.backendInstance;
        if (!nullableText(backendInstance) || (value.nodeId != null && !text(value.nodeId))) throw Error('Invalid chat account identity');
        if (value.usage != null && (typeof value.usage !== 'object' || Array.isArray(value.usage))) throw Error('Invalid chat usage');
        const identity = { source: 'chat' as const, profile: current, project_provider: null, repo: null, work_id: work,
          backend: value.backend, backend_instance: backendInstance, account_label: null, auth_source_label: null, quota_pool: null, node_id: (value.nodeId as string | undefined) ?? null,
          requested_model: null, effective_model: (value.model as string | null | undefined) ?? null, actual_model: null, model_provider: null,
          usage_classification: null, day: new Date(value.timestamp).toISOString().slice(0, 10) };
        const key = JSON.stringify(identity);
        const row = groups.get(key) ?? { ...identity, chat_turns: 0, dispatch_attempts: 0, legacy_dispatches: 0, measurements: emptyUsageMeasurements() };
        row.chat_turns += 1;
        const usage = (value.usage ?? {}) as Record<string, unknown>;
        // Chat records expose API-equivalent estimates, not actual billed cost.
        addUsageMeasurements(row.measurements, observeUsage({ ...usage, actual_cost_usd: null }));
        groups.set(key, row);
      }
    }
  }
  return [...groups.values()];
}

/** One coherent window and one source declaration for dashboard and JSON export.
 * A failed source keeps its warning while successful source measurements survive. */
export async function recordedUsageReport(options: Options, deps: Dependencies = {}): Promise<RecordedUsageReport> {
  const now = (deps.now ?? Date.now)();
  let since = now - options.days * DAY;
  let until = now;
  let dispatch: RecordedUsageRow[] = [];
  let chat: RecordedUsageRow[] = [];
  const sources: RecordedUsageReport['sources'] = {
    chat: { status: 'unavailable', message: 'Chat usage could not be read. Check the session logs and retry.' },
    dispatch: { status: 'unavailable', message: 'Dispatch usage could not be read. Check or update the node CLI and retry.' }
  };
  try {
    const report = await (deps.readDispatch ?? runDispatchUsageReport)({ profile: options.profile, since: `${options.days}d` });
    const start = Date.parse(report.since);
    const end = Date.parse(report.generated_at);
    if (!Number.isFinite(start) || !Number.isFinite(end) || end < start || Math.abs(end - now) > 5 * 60_000
      || Math.abs(end - start - options.days * DAY) > 5_000) throw Error('Invalid usage source window');
    dispatch = dispatchRows(report, options.profile);
    since = start; until = end;
    sources.dispatch = { status: 'available', excluded_preflight_entries: report.excluded_preflight_entries };
  } catch { /* Source status is returned; private CLI output is never sent to the page. */ }
  try {
    chat = await chatRows(options.profile, since, until, deps.stateDir);
    sources.chat = { status: 'available' };
  } catch { /* An unreadable log is not an empty successful source. */ }
  const rows = [...chat, ...dispatch].sort((a, b) => b.day.localeCompare(a.day) || a.profile.localeCompare(b.profile));
  const totals: UsageMeasurements = emptyUsageMeasurements();
  for (const row of rows) addUsageMeasurements(totals, row.measurements);
  return { schema_version: 1, scope: 'recorded-ledger-and-chat', profile: options.profile ?? null,
    since: new Date(since).toISOString(), generated_at: new Date(until).toISOString(), sources, rows, totals };
}
