import { existsSync, readFileSync } from 'node:fs';
import { writeFileAtomic } from '../../atomicFile.js';

/** Reserve receipts before acknowledgement. A full store refuses work instead of forgetting live receipts. */
export class EventDedupe {
  private events: Map<string, number> | undefined;
  constructor(private readonly capacity = 10_000, private readonly ttlMs = 86_400_000,
    private readonly now = Date.now, private readonly path?: string) {}

  reserve(key: string): 'new' | 'duplicate' | 'full' {
    const events = this.load();
    const now = this.now();
    for (const [id, expires] of events) {
      if (expires <= now) events.delete(id);
    }
    if (events.has(key)) return 'duplicate';
    if (events.size >= this.capacity) return 'full';
    const next = new Map(events).set(key, now + this.ttlMs);
    if (this.path) writeFileAtomic(this.path, JSON.stringify([...next]));
    this.events = next;
    return 'new';
  }

  private load(): Map<string, number> {
    if (this.events) return this.events;
    if (!this.path || !existsSync(this.path)) return this.events = new Map();
    const rows: unknown = JSON.parse(readFileSync(this.path, 'utf8'));
    if (!Array.isArray(rows) || rows.length > this.capacity || rows.some(row => !Array.isArray(row) || row.length !== 2
      || typeof row[0] !== 'string' || !/^[a-f0-9]{64}$/.test(row[0]) || !Number.isFinite(row[1]))) {
      throw new Error('Invalid GitLab event receipts');
    }
    return this.events = new Map(rows);
  }
}
