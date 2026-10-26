import { existsSync, readFileSync } from 'node:fs';
import { writeFileAtomic } from '../../atomicFile.js';

export type QueuedEvent = { body: unknown; bot: string; panelUrl: string; pipelineEnabled: boolean; eventUuid?: string };
type Receipt = { expires: number; pending?: QueuedEvent };

/** Reserve receipts before acknowledgement. A full store refuses work instead of forgetting live receipts. */
export class EventDedupe {
  private events: Map<string, Receipt> | undefined;
  constructor(private readonly capacity = 10_000, private readonly ttlMs = 86_400_000,
    private readonly now = Date.now, private readonly path?: string) {}

  reserve(key: string, pending?: QueuedEvent): 'new' | 'duplicate' | 'full' {
    const events = this.load();
    const now = this.now();
    for (const [id, receipt] of events) {
      if (!receipt.pending && receipt.expires <= now) events.delete(id);
    }
    if (events.has(key)) return 'duplicate';
    if (events.size >= this.capacity) return 'full';
    const next = new Map(events).set(key, { expires: now + this.ttlMs, pending });
    if (this.path) writeFileAtomic(this.path, JSON.stringify([...next]));
    this.events = next;
    return 'new';
  }

  queued(): Array<[string, QueuedEvent]> {
    return [...this.load()].flatMap(([key, receipt]) => receipt.pending ? [[key, receipt.pending] as [string, QueuedEvent]] : []);
  }

  complete(key: string): void {
    const events = this.load();
    if (!events.has(key)) return;
    const next = new Map(events).set(key, { expires: this.now() + this.ttlMs });
    if (this.path) writeFileAtomic(this.path, JSON.stringify([...next]));
    this.events = next;
  }

  private load(): Map<string, Receipt> {
    if (this.events) return this.events;
    if (!this.path || !existsSync(this.path)) return this.events = new Map();
    const rows: unknown = JSON.parse(readFileSync(this.path, 'utf8'));
    if (!Array.isArray(rows) || rows.length > this.capacity) throw new Error('Invalid GitLab event receipts');
    const events = new Map<string, Receipt>();
    for (const row of rows) {
      if (!Array.isArray(row) || row.length !== 2 || typeof row[0] !== 'string' || !/^[a-f0-9]{64}$/.test(row[0])) {
        throw new Error('Invalid GitLab event receipts');
      }
      // Read receipts written before the durable queue was introduced.
      const receipt: Receipt = typeof row[1] === 'number' ? { expires: row[1] } : row[1];
      if (!receipt || !Number.isFinite(receipt.expires) || (receipt.pending &&
        (typeof receipt.pending.bot !== 'string' || typeof receipt.pending.panelUrl !== 'string'
          || typeof receipt.pending.pipelineEnabled !== 'boolean' || !('body' in receipt.pending)))) {
        throw new Error('Invalid GitLab event receipts');
      }
      events.set(row[0], receipt);
    }
    return this.events = events;
  }
}
