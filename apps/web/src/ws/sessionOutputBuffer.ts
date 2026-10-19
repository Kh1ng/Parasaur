/**
 * Bounded per-session stdout/stderr retention for the web client (#1558).
 *
 * The server streams every `session.stdout`/`session.stderr` line to every
 * client, so a long-lived tab must not keep all of it. Each stream keeps at
 * most MAX_OUTPUT_LINES_PER_STREAM lines and MAX_OUTPUT_CHARS_PER_STREAM
 * characters, dropping the oldest first and flagging the stream as
 * truncated. Finished sessions keep their output so the session detail view
 * can still show it, but only the MAX_FINISHED_SESSIONS most recently
 * finished ones; MAX_RETAINED_SESSIONS caps every entry, finished or not.
 *
 * Appends are O(1) amortized: lines go into an array and the dropped head is
 * compacted away in bulk instead of copying one ever-growing string.
 */
import type { SessionId } from '@git-agent-harness/contracts';

export const MAX_OUTPUT_LINES_PER_STREAM = 2000;
/** UTF-16 code units, i.e. ~256 KB of ASCII output per stream. */
export const MAX_OUTPUT_CHARS_PER_STREAM = 256 * 1024;
export const MAX_FINISHED_SESSIONS = 20;
export const MAX_RETAINED_SESSIONS = 100;

export type OutputStreamName = 'stdout' | 'stderr';

export interface SessionOutputText {
  /** Retained lines, each followed by '\n'. */
  stdout: string;
  stderr: string;
  /** True once either stream dropped earlier output. */
  truncated: boolean;
}

class OutputStream {
  private lines: string[] = [];
  private start = 0;
  private chars = 0;
  truncated = false;

  append(line: string): void {
    if (line.length > MAX_OUTPUT_CHARS_PER_STREAM) {
      line = line.slice(-MAX_OUTPUT_CHARS_PER_STREAM);
      this.truncated = true;
    }
    this.lines.push(line);
    this.chars += line.length;
    while (this.lines.length - this.start > MAX_OUTPUT_LINES_PER_STREAM || this.chars > MAX_OUTPUT_CHARS_PER_STREAM) {
      this.chars -= this.lines[this.start].length;
      this.start += 1;
      this.truncated = true;
    }
    // Drop the dead head in bulk once it outweighs the live tail.
    if (this.start > MAX_OUTPUT_LINES_PER_STREAM && this.start * 2 > this.lines.length) {
      this.lines = this.lines.slice(this.start);
      this.start = 0;
    }
  }

  retained(): string[] {
    return this.lines.slice(this.start);
  }

  text(): string {
    return this.lines.length > this.start ? this.retained().join('\n') + '\n' : '';
  }
}

interface Entry {
  stdout: OutputStream;
  stderr: OutputStream;
}

export class SessionOutputBuffer {
  // Map iteration order is insertion order: oldest entry first.
  private entries = new Map<SessionId, Entry>();
  // Oldest finish first.
  private finished = new Set<SessionId>();

  append(sessionId: SessionId, stream: OutputStreamName, line: string): void {
    let entry = this.entries.get(sessionId);
    if (!entry) {
      entry = { stdout: new OutputStream(), stderr: new OutputStream() };
      this.entries.set(sessionId, entry);
      this.enforceSessionCap();
    }
    entry[stream].append(line);
  }

  /** A session stopped (or vanished from the server's list): keep its output
   * only while it is among the most recently finished. */
  markFinished(sessionId: SessionId): void {
    if (!this.entries.has(sessionId)) return;
    this.finished.delete(sessionId);
    this.finished.add(sessionId);
    while (this.finished.size > MAX_FINISHED_SESSIONS) {
      const oldest = this.finished.values().next().value as SessionId;
      this.delete(oldest);
    }
  }

  /** A finished session is running again (e.g. resumed). */
  markActive(sessionId: SessionId): void {
    this.finished.delete(sessionId);
  }

  /** The server's session list was replaced; anything not in it has ended. */
  retainLive(liveSessionIds: Iterable<SessionId>): void {
    const live = new Set(liveSessionIds);
    for (const sessionId of [...this.entries.keys()]) {
      if (!live.has(sessionId) && !this.finished.has(sessionId)) this.markFinished(sessionId);
    }
  }

  has(sessionId: SessionId): boolean {
    return this.entries.has(sessionId);
  }

  sessionIds(): SessionId[] {
    return [...this.entries.keys()];
  }

  lines(sessionId: SessionId, stream: OutputStreamName): string[] {
    return this.entries.get(sessionId)?.[stream].retained() ?? [];
  }

  text(sessionId: SessionId): SessionOutputText | undefined {
    const entry = this.entries.get(sessionId);
    if (!entry) return undefined;
    return {
      stdout: entry.stdout.text(),
      stderr: entry.stderr.text(),
      truncated: entry.stdout.truncated || entry.stderr.truncated
    };
  }

  delete(sessionId: SessionId): void {
    this.entries.delete(sessionId);
    this.finished.delete(sessionId);
  }

  clear(): void {
    this.entries.clear();
    this.finished.clear();
  }

  private enforceSessionCap(): void {
    while (this.entries.size > MAX_RETAINED_SESSIONS) {
      const oldestFinished = this.finished.values().next();
      this.delete(oldestFinished.done ? this.entries.keys().next().value as SessionId : oldestFinished.value);
    }
  }
}
