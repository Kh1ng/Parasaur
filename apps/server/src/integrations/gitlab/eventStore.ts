import { existsSync, readFileSync } from 'node:fs';
import { writeFileAtomic } from '../../atomicFile.js';
import { classifyEvent, record } from './classifier.js';

export const MAX_DISPATCH_ATTEMPTS = 3;
export const DISPATCH_RETRY_MS = 30_000;
export const DISPATCH_LEASE_MS = 3_600_000;

function configuredQueueCapacity(): number {
  const configured = Number(process.env.GAH_GITLAB_EVENTS_QUEUE_CAPACITY);
  return Number.isSafeInteger(configured) && configured > 0 ? configured : 256;
}

export type GitLabDelivery = {
  key: string;
  body: unknown;
  eventUuid?: string;
  bot: string;
  pipelineEnabled: boolean;
  panelUrl: string;
};

export type DispatchMarker = { requestId: string; sessionId?: string };
type QueueEntry = GitLabDelivery & { attempts: number; retryAt: number; started?: DispatchMarker };
type EventState = {
  version: 1;
  receipts: Array<[string, number]>;
  pending: QueueEntry[];
  deadLetters: QueueEntry[];
  acknowledged: string[];
};

/** Atomically accept work with its receipt. Pending and dead-letter work never expires or gets evicted. */
export class GitLabEventStore {
  private state: EventState | undefined;
  constructor(private readonly capacity = 10_000, private readonly ttlMs = 86_400_000,
    private readonly now = Date.now, private readonly path?: string, private readonly queueCapacity = configuredQueueCapacity(),
    private readonly deadLetterCapacity = 64) {}

  enqueue(delivery: GitLabDelivery): 'new' | 'duplicate' | 'full' | 'queue_full' {
    const state = this.load();
    const retained = new Set([...state.pending, ...state.deadLetters].map(entry => entry.key).concat(state.acknowledged));
    const receipts = state.receipts.filter(([key, expires]) => expires > this.now() || retained.has(key));
    if (receipts.some(([key]) => key === delivery.key)) return 'duplicate';
    if (state.pending.length >= this.queueCapacity) return 'queue_full';
    if (receipts.length >= this.capacity) return 'full';
    const entry = { ...delivery, attempts: 0, retryAt: 0 };
    if (!this.validEntry(entry, false)) throw new Error('Invalid GitLab delivery');
    this.save({ ...state, receipts: [...receipts, [delivery.key, this.now() + this.ttlMs]],
      pending: [...state.pending, entry] });
    return 'new';
  }

  ready(): QueueEntry[] {
    return this.load().pending.filter(entry => entry.retryAt <= this.now());
  }

  /** Completed or discarded keys whose fleet receipts still need release. */
  acknowledged(): string[] {
    return [...this.load().acknowledged];
  }

  /** Forget release work only after the fleet receipt is durably unpinned. */
  receiptReleased(key: string): void {
    const state = this.load();
    this.save({ ...state, acknowledged: state.acknowledged.filter(id => id !== key) });
  }

  /** Reserve identity and consume one attempt before dispatch; false means the budget is exhausted. */
  begin(key: string): boolean {
    const state = this.load();
    const entry = state.pending.find(entry => entry.key === key);
    if (!entry) throw new Error('Missing GitLab delivery');
    if (entry.attempts >= MAX_DISPATCH_ATTEMPTS) return false;
    const started = { requestId: `gitlab-${key}` };
    this.save({ ...state, pending: state.pending.map(entry => entry.key === key
      ? { ...entry, started, attempts: entry.attempts + 1, retryAt: this.now() + DISPATCH_LEASE_MS } : entry) });
    return true;
  }

  retryAfter(): number {
    const leases = this.load().pending.map(entry => entry.retryAt).filter(time => time > this.now());
    return leases.length ? Math.max(1, Math.ceil((Math.min(...leases) - this.now()) / 1000)) : 30;
  }

  /** Return operational metadata only, never accepted issue/note payloads. */
  summary() {
    const summarize = (entry: QueueEntry) => ({ id: entry.key, attempts: entry.attempts,
      retryAt: entry.retryAt, started: entry.started });
    const state = this.load();
    return { pending: state.pending.map(summarize), deadLetters: state.deadLetters.map(summarize), acknowledged: this.acknowledged(),
      capacity: this.queueCapacity, deadLetterCapacity: this.deadLetterCapacity };
  }

  /** Atomically recover a dead letter while preserving its receipt and dispatch identity. */
  recover(key: string, action: 'retry' | 'discard'): 'ok' | 'not_found' | 'queue_full' {
    const state = this.load();
    const entry = state.deadLetters.find(entry => entry.key === key);
    if (!entry) return 'not_found';
    if (action === 'retry' && state.pending.length >= this.queueCapacity) return 'queue_full';
    this.save({ ...state, deadLetters: state.deadLetters.filter(entry => entry.key !== key),
      pending: action === 'retry' ? [...state.pending, { ...entry, attempts: 0, retryAt: 0 }] : state.pending,
      acknowledged: action === 'discard' ? [...state.acknowledged, key] : state.acknowledged,
      receipts: state.receipts.map(([id, expires]) => [id, id === key ? this.now() + this.ttlMs : expires]) });
    return 'ok';
  }

  /** Atomically replace completed work with a durable obligation to release its fleet receipt. */
  complete(key: string): void {
    const state = this.load();
    if (!state.pending.some(entry => entry.key === key)) return;
    this.save({ ...state, pending: state.pending.filter(entry => entry.key !== key),
      acknowledged: [...state.acknowledged, key],
      receipts: state.receipts.map(([id, expires]) => [id, id === key ? this.now() + this.ttlMs : expires]) });
  }

  /** Schedule a reserved attempt for retry; return true only on transition to dead-letter. */
  failed(key: string): boolean {
    const state = this.load();
    const entry = state.pending.find(entry => entry.key === key);
    if (!entry) return false;
    const next = { ...entry, retryAt: this.now() + DISPATCH_RETRY_MS };
    // Preserve poison work when the separate dead-letter bound is full.
    const dead = next.attempts >= MAX_DISPATCH_ATTEMPTS && state.deadLetters.length < this.deadLetterCapacity;
    this.save({ ...state,
      pending: dead ? state.pending.filter(entry => entry.key !== key) : state.pending.map(entry => entry.key === key ? next : entry),
      deadLetters: dead ? [...state.deadLetters, next] : state.deadLetters });
    return dead;
  }

  private save(state: EventState): void {
    if (this.path) writeFileAtomic(this.path, JSON.stringify(state));
    this.state = state;
  }

  private load(): EventState {
    if (this.state) return this.state;
    if (!this.path || !existsSync(this.path)) return this.state = { version: 1, receipts: [], pending: [], deadLetters: [], acknowledged: [] };
    const payload: unknown = JSON.parse(readFileSync(this.path, 'utf8'));
    // Upgrade the original receipt-only array on the next successful write.
    const state = Array.isArray(payload) ? { version: 1, receipts: payload, pending: [], deadLetters: [], acknowledged: [] } : record(payload);
    if (state.acknowledged === undefined) state.acknowledged = [];
    const { receipts, pending, deadLetters, acknowledged } = state;
    // A lower pending capacity must not invalidate previously accepted deliveries.
    if (state.version !== 1 || !Array.isArray(receipts) || receipts.length > this.capacity
      || receipts.some(row => !Array.isArray(row) || row.length !== 2 || typeof row[0] !== 'string'
        || !/^[a-f0-9]{64}$/.test(row[0]) || !Number.isFinite(row[1]))
      || new Set(receipts.map(row => row[0])).size !== receipts.length
      || !Array.isArray(pending) || !Array.isArray(deadLetters) || deadLetters.length > this.deadLetterCapacity
      || !Array.isArray(acknowledged) || acknowledged.some(key => typeof key !== 'string' || !/^[a-f0-9]{64}$/.test(key))
      || pending.some(entry => !this.validEntry(entry, false)) || deadLetters.some(entry => !this.validEntry(entry, true))) {
      throw new Error('Invalid GitLab event store');
    }
    const keys = [...pending, ...deadLetters].map(entry => entry.key).concat(acknowledged);
    if (new Set(keys).size !== keys.length || keys.some(key => !receipts.some(row => row[0] === key))) {
      throw new Error('Invalid GitLab event store');
    }
    return this.state = state as EventState;
  }

  private validEntry(value: unknown, dead: boolean): value is QueueEntry {
    const entry = record(value);
    const started = entry.started === undefined ? undefined : record(entry.started);
    if (typeof entry.key !== 'string' || !/^[a-f0-9]{64}$/.test(entry.key)
      || typeof entry.bot !== 'string' || typeof entry.pipelineEnabled !== 'boolean'
      || (entry.eventUuid !== undefined && typeof entry.eventUuid !== 'string')
      || (started && (started.requestId !== `gitlab-${entry.key}`
        || (started.sessionId !== undefined && (typeof started.sessionId !== 'string' || !started.sessionId))))
      || typeof entry.panelUrl !== 'string' || !Number.isSafeInteger(entry.attempts) || !Number.isFinite(entry.retryAt)
      || Number(entry.attempts) < 0 || Number(entry.attempts) > MAX_DISPATCH_ATTEMPTS
      || (dead && Number(entry.attempts) !== MAX_DISPATCH_ATTEMPTS)
      || Buffer.byteLength(JSON.stringify(entry.body) ?? '') > 102_400) return false;
    try {
      const url = new URL(entry.panelUrl);
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) return false;
      return classifyEvent(entry.body, entry.bot, entry.pipelineEnabled, entry.eventUuid as string | undefined)?.key === entry.key;
    } catch { return false; }
  }
}
