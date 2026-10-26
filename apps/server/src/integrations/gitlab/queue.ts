import type { Session } from '@git-agent-harness/contracts';
import { classifyEvent, type GitLabEvent } from './classifier.js';
import { DISPATCH_RETRY_MS, GitLabEventStore, MAX_DISPATCH_ATTEMPTS, type DispatchMarker } from './eventStore.js';

export type GitLabDispatchOptions = { bot: string; panelUrl: string; requestId?: string };
export type GitLabDispatch = (event: GitLabEvent, options: GitLabDispatchOptions) => Promise<void>;
export type FindGitLabDispatch = (identity: DispatchMarker) => Promise<Pick<Session, 'id' | 'leaseState'> | undefined>;

/** Replay accepted deliveries independently of HTTP requests; serialize drains and isolate poison work. */
export class GitLabEventQueue {
  private running?: Promise<void>;
  private scheduled?: NodeJS.Immediate;
  private timer?: NodeJS.Timeout;
  private stopped = false;
  private storeFailed = false;
  private releaseFailed = false;

  constructor(private readonly store: GitLabEventStore, private readonly dispatch: GitLabDispatch,
    private readonly findDispatch: FindGitLabDispatch = async () => undefined,
    private readonly releaseReceipt: (requestId: string) => void = () => {}) {}

  /** Start only once fleet dispatch is initialized. Provider calls run in the background. */
  start(): void {
    this.stopped = false;
    this.timer ??= setInterval(() => this.wake(), DISPATCH_RETRY_MS);
    this.timer.unref();
    this.wake();
  }

  stop(): void {
    this.stopped = true;
    clearInterval(this.timer);
    clearImmediate(this.scheduled);
    this.timer = undefined;
    this.scheduled = undefined;
  }

  wake(): void {
    if (this.stopped || this.scheduled) return;
    this.scheduled = setImmediate(() => {
      this.scheduled = undefined;
      void this.drain();
    });
  }

  /** Also used by restart tests to wait until the current replay has settled. */
  drain(): Promise<void> {
    if (this.running) return this.running;
    this.running = this.replay().catch(() => {
      if (!this.storeFailed) console.error('[gitlab-events] Event store unavailable; queued work retained.');
      this.storeFailed = true;
    }).finally(() => { this.running = undefined; });
    return this.running;
  }

  /** Owner recovery uses the same replay path and dispatch policy as hook acceptance. */
  recover(key: string, action: 'retry' | 'discard') {
    const result = this.store.recover(key, action);
    if (result === 'ok') {
      if (action === 'retry') this.wake();
      else this.release(key);
    }
    return result;
  }

  private release(key: string): void {
    try { this.releaseReceipt(`gitlab-${key}`); }
    catch {
      if (!this.releaseFailed) console.error('[gitlab-events] Fleet receipt release unavailable; acknowledgements retained for replay.');
      this.releaseFailed = true;
      return;
    }
    this.releaseFailed = false;
    this.store.receiptReleased(key);
  }

  private complete(key: string): void {
    this.store.complete(key);
    // Replay retains the release obligation across either store's write failures.
    this.release(key);
  }

  private async replay(): Promise<void> {
    for (const key of this.store.acknowledged()) {
      if (this.stopped) return;
      this.release(key);
    }
    while (!this.stopped) {
      const ready = this.store.ready();
      if (!ready.length) break;
      for (const entry of ready) {
        if (this.stopped) return;
        const event = classifyEvent(entry.body, entry.bot, entry.pipelineEnabled, entry.eventUuid);
        // Reservation and budget survive a crash anywhere in the dispatcher.
        const reserved = this.store.begin(entry.key);
        let failed = false;
        try {
          const previous = entry.started ? await this.findDispatch(entry.started) : undefined;
          if (previous?.leaseState !== 'running' && previous?.leaseState !== 'terminal') {
            if (!reserved || (previous && previous.leaseState !== 'expired')) failed = true;
            else if (event) await this.dispatch(event, { bot: entry.bot, panelUrl: entry.panelUrl, requestId: `gitlab-${entry.key}` });
          }
        } catch { failed = true; }
        if (failed) {
          if (this.store.failed(entry.key)) {
            console.error(`[gitlab-events] Event ${entry.key} moved to dead letters after ${MAX_DISPATCH_ATTEMPTS} dispatch failures.`);
          }
        } else {
          this.complete(entry.key);
        }
      }
    }
    this.storeFailed = false;
  }
}
