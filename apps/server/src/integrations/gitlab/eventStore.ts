import { existsSync, readFileSync } from 'node:fs';
import { writeFileAtomic } from '../../atomicFile.js';
import { classifyEvent, record } from './classifier.js';

export const MAX_DISPATCH_ATTEMPTS = 3;
export const DISPATCH_RETRY_MS = 30_000;

export type GitLabDelivery = {
  key: string;
  body: unknown;
  eventUuid?: string;
  bot: string;
  pipelineEnabled: boolean;
  panelUrl: string;
};

type QueueEntry = GitLabDelivery & { attempts: number; retryAt: number };
type EventState = {
  version: 1;
  receipts: Array<[string, number]>;
  pending: QueueEntry[];
  deadLetters: QueueEntry[];
};

/** Atomically accept work with its receipt. Pending and dead-letter work never expires or gets evicted. */
export class GitLabEventStore {
  private state: EventState | undefined;
  constructor(private readonly capacity = 10_000, private readonly ttlMs = 86_400_000,
    private readonly now = Date.now, private readonly path?: string, private readonly queueCapacity = 64) {}

  enqueue(delivery: GitLabDelivery): 'new' | 'duplicate' | 'full' | 'queue_full' {
    const state = this.load();
    const retained = new Set([...state.pending, ...state.deadLetters].map(entry => entry.key));
    const receipts = state.receipts.filter(([key, expires]) => expires > this.now() || retained.has(key));
    if (receipts.some(([key]) => key === delivery.key)) return 'duplicate';
    if (retained.size >= this.queueCapacity) return 'queue_full';
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

  /** Called only after the dispatcher has recorded the start or rejected it under current policy. */
  complete(key: string): void {
    const state = this.load();
    this.save({ ...state, pending: state.pending.filter(entry => entry.key !== key),
      receipts: state.receipts.map(([id, expires]) => [id, id === key ? this.now() + this.ttlMs : expires]) });
  }

  /** Persist a failed attempt; return true only on the transition to dead-letter. */
  failed(key: string): boolean {
    const state = this.load();
    const entry = state.pending.find(entry => entry.key === key);
    if (!entry) return false;
    const next = { ...entry, attempts: entry.attempts + 1, retryAt: this.now() + DISPATCH_RETRY_MS };
    const dead = next.attempts >= MAX_DISPATCH_ATTEMPTS;
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
    if (!this.path || !existsSync(this.path)) return this.state = { version: 1, receipts: [], pending: [], deadLetters: [] };
    const payload: unknown = JSON.parse(readFileSync(this.path, 'utf8'));
    // Upgrade the original receipt-only array on the next successful write.
    const state = Array.isArray(payload) ? { version: 1, receipts: payload, pending: [], deadLetters: [] } : record(payload);
    const { receipts, pending, deadLetters } = state;
    if (state.version !== 1 || !Array.isArray(receipts) || receipts.length > this.capacity
      || receipts.some(row => !Array.isArray(row) || row.length !== 2 || typeof row[0] !== 'string'
        || !/^[a-f0-9]{64}$/.test(row[0]) || !Number.isFinite(row[1]))
      || new Set(receipts.map(row => row[0])).size !== receipts.length
      || !Array.isArray(pending) || !Array.isArray(deadLetters) || pending.length + deadLetters.length > this.queueCapacity
      || pending.some(entry => !this.validEntry(entry, false)) || deadLetters.some(entry => !this.validEntry(entry, true))) {
      throw new Error('Invalid GitLab event store');
    }
    const keys = [...pending, ...deadLetters].map(entry => entry.key);
    if (new Set(keys).size !== keys.length || keys.some(key => !receipts.some(row => row[0] === key))) {
      throw new Error('Invalid GitLab event store');
    }
    return this.state = state as EventState;
  }

  private validEntry(value: unknown, dead: boolean): value is QueueEntry {
    const entry = record(value);
    if (typeof entry.key !== 'string' || !/^[a-f0-9]{64}$/.test(entry.key)
      || typeof entry.bot !== 'string' || typeof entry.pipelineEnabled !== 'boolean'
      || (entry.eventUuid !== undefined && typeof entry.eventUuid !== 'string')
      || typeof entry.panelUrl !== 'string' || !Number.isSafeInteger(entry.attempts) || !Number.isFinite(entry.retryAt)
      || Number(entry.attempts) < 0 || Number(entry.attempts) > MAX_DISPATCH_ATTEMPTS
      || (Number(entry.attempts) === MAX_DISPATCH_ATTEMPTS) !== dead
      || Buffer.byteLength(JSON.stringify(entry.body) ?? '') > 102_400) return false;
    try {
      const url = new URL(entry.panelUrl);
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) return false;
      return classifyEvent(entry.body, entry.bot, entry.pipelineEnabled, entry.eventUuid as string | undefined)?.key === entry.key;
    } catch { return false; }
  }
}
