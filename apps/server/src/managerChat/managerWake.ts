/** Preserve wake ordering while a failed instruction retries, with one log per cause transition. */
export class ManagerWakeQueue {
  private pending = new Map<string, Promise<void>>();

  constructor(private sleep = (ms: number): Promise<void> => new Promise(resolve => {
    const timer = setTimeout(resolve, ms);
    timer.unref();
  })) {}

  enqueue(key: string, run: (retryCount: number) => Promise<unknown>): Promise<void> {
    const turn = (this.pending.get(key) ?? Promise.resolve()).then(async () => {
      let delay = 5_000;
      let lastError: string | undefined;
      let retryCount = 0;
      for (;;) {
        try {
          await run(retryCount);
          return;
        } catch (error) {
          const message = error instanceof Error ? error.message : String(error);
          if (lastError !== message) {
            lastError = message;
            console.error('[managerChat] wake failed:', key, message);
          }
          await this.sleep(delay);
          retryCount++;
          delay = Math.min(delay * 2, 300_000);
        }
      }
    });
    this.pending.set(key, turn);
    void turn.finally(() => {
      if (this.pending.get(key) === turn) this.pending.delete(key);
    });
    return turn;
  }
}
