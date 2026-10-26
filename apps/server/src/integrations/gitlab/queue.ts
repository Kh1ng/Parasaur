import { classifyEvent, type GitLabEvent } from './classifier.js';
import { DISPATCH_RETRY_MS, GitLabEventStore, MAX_DISPATCH_ATTEMPTS, type DispatchMarker } from './eventStore.js';

export type GitLabDispatchOptions = { bot: string; panelUrl: string; onStarted?: (sessionId: string) => void };
export type GitLabDispatch = (event: GitLabEvent, options: GitLabDispatchOptions) => Promise<void>;
export type FindGitLabDispatch = (identity: DispatchMarker) => Promise<{ id: string } | undefined>;

/** Replay accepted deliveries independently of HTTP requests; serialize drains and isolate poison work. */
export class GitLabEventQueue {
  private running?: Promise<void>;
  private scheduled?: NodeJS.Immediate;
  private timer?: NodeJS.Timeout;
  private stopped = false;
  private storeFailed = false;

  constructor(private readonly store: GitLabEventStore, private readonly dispatch: GitLabDispatch,
    private readonly findDispatch: FindGitLabDispatch = async () => undefined) {}

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

  private async replay(): Promise<void> {
    while (!this.stopped) {
      const ready = this.store.ready();
      if (!ready.length) break;
      for (const entry of ready) {
        if (this.stopped) return;
        const event = classifyEvent(entry.body, entry.bot, entry.pipelineEnabled, entry.eventUuid);
        // A saved session is a durable start receipt even after fleet retention expires.
        // Otherwise inspect the fleet request identity before attempting another start.
        if (entry.started) {
          const previous = await this.findDispatch(entry.started);
          if (entry.started.sessionId || previous) {
            this.store.complete(entry.key);
            continue;
          }
        }
        this.store.begin(entry.key);
        let failed = false;
        try {
          if (event) await this.dispatch(event, { bot: entry.bot, panelUrl: entry.panelUrl, onStarted: id => this.store.started(entry.key, id) });
        } catch { failed = true; }
        if (failed) {
          if (this.store.failed(entry.key)) {
            console.error(`[gitlab-events] Event ${entry.key} moved to dead letters after ${MAX_DISPATCH_ATTEMPTS} dispatch failures.`);
          }
        } else {
          this.store.complete(entry.key);
        }
      }
    }
    this.storeFailed = false;
  }
}
