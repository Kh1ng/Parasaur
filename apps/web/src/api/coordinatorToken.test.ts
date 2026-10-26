import { coordinatorToken, requestSessionHandoff, saveCoordinatorToken } from './coordinatorToken.js';

/** The embed iframe and the sign-in tab are different browsing contexts:
 * separate sessionStorage, shared localStorage (same origin). The fakes
 * model exactly that boundary — two windows over one localStorage. */
class FakeStorage {
  private map = new Map<string, string>();
  getItem(key: string): string | null { return this.map.has(key) ? this.map.get(key)! : null; }
  setItem(key: string, value: string): void { this.map.set(key, value); }
  removeItem(key: string): void { this.map.delete(key); }
  clear(): void { this.map.clear(); }
}

class FakeWindow {
  readonly sessionStorage = new FakeStorage();
  constructor(readonly localStorage: FakeStorage) {}
  private readonly listeners = new Map<string, Set<(event: Event & { key?: string | null }) => void>>();
  addEventListener(type: string, listener: (event: Event & { key?: string | null }) => void): void {
    if (!this.listeners.has(type)) this.listeners.set(type, new Set());
    this.listeners.get(type)!.add(listener);
  }
  removeEventListener(type: string, listener: (event: Event & { key?: string | null }) => void): void {
    this.listeners.get(type)?.delete(listener);
  }
  dispatchEvent(event: Event & { key?: string | null }): boolean {
    for (const listener of this.listeners.get(event.type) ?? []) listener(event);
    return true;
  }
}

function withWindow(window: FakeWindow, run: () => void): void {
  const previous = (globalThis as Record<string, unknown>).window;
  (globalThis as Record<string, unknown>).window = window;
  try {
    run();
  } finally {
    if (previous === undefined) delete (globalThis as Record<string, unknown>).window;
    else (globalThis as Record<string, unknown>).window = previous;
  }
}

const HANDOFF_KEY = 'gah.coordinatorTokenHandoff';
const WANTED_KEY = 'gah.coordinatorTokenWanted';
const TOKEN_KEY = 'gah.coordinatorToken';

test('a sign-in that nobody waits for never touches localStorage', () => {
  const shared = new FakeStorage();
  const window = new FakeWindow(shared);
  withWindow(window, () => {
    saveCoordinatorToken('owner-token');
    expect(window.sessionStorage.getItem(TOKEN_KEY)).toBe('owner-token');
    expect(shared.getItem(HANDOFF_KEY)).toBeNull();
    expect(shared.getItem(WANTED_KEY)).toBeNull();
    saveCoordinatorToken('');
    expect(window.sessionStorage.getItem(TOKEN_KEY)).toBeNull();
  });
});

test('a waiting embed view adopts the credential another tab hands over, then clears the handoff', () => {
  const shared = new FakeStorage();
  const embedWindow = new FakeWindow(shared);
  const signInWindow = new FakeWindow(shared);
  const adopted: string[] = [];
  let stop = () => {};
  withWindow(embedWindow, () => {
    stop = requestSessionHandoff(token => adopted.push(token));
    expect(shared.getItem(WANTED_KEY)).toBe('1');
    // The sign-in happens in another browsing context: its own sessionStorage,
    // the same shared localStorage. The wanted marker routes the publish.
    withWindow(signInWindow, () => {
      saveCoordinatorToken('owner-token-from-sign-in-tab');
      expect(signInWindow.sessionStorage.getItem(TOKEN_KEY)).toBe('owner-token-from-sign-in-tab');
    });
    // The sign-in tab never touched the embed context's sessionStorage...
    expect(coordinatorToken()).toBe('');
    // ...but the handoff waits in shared storage. The browser fires the
    // storage event in the other documents of the origin.
    const event = new Event('storage') as Event & { key?: string | null };
    event.key = HANDOFF_KEY;
    embedWindow.dispatchEvent(event);
    expect(adopted).toEqual(['owner-token-from-sign-in-tab']);
    // The adopter consumed the handoff: nothing sensitive stays behind.
    expect(shared.getItem(HANDOFF_KEY)).toBeNull();
    expect(shared.getItem(WANTED_KEY)).toBeNull();
    // Picking the session up stores it in this context's own tab-scoped
    // storage, exactly like a direct sign-in would have.
    saveCoordinatorToken(adopted[0]);
    expect(coordinatorToken()).toBe('owner-token-from-sign-in-tab');
    // The mirror-in does not loop: no waiter is marked anymore.
    expect(shared.getItem(HANDOFF_KEY)).toBeNull();
    stop();
    expect(shared.getItem(WANTED_KEY)).toBeNull();
  });
});

test('a handoff published before the listener registered is adopted on registration', () => {
  const shared = new FakeStorage();
  const embedWindow = new FakeWindow(shared);
  const adopted: string[] = [];
  withWindow(embedWindow, () => {
    embedWindow.localStorage.setItem(WANTED_KEY, '1');
    embedWindow.localStorage.setItem(HANDOFF_KEY, 'already-waiting');
    requestSessionHandoff(token => adopted.push(token))();
    expect(adopted).toEqual(['already-waiting']);
    expect(embedWindow.localStorage.getItem(HANDOFF_KEY)).toBeNull();
  });
});

test('storage events for other keys and empty handoffs adopt nothing', () => {
  const shared = new FakeStorage();
  const embedWindow = new FakeWindow(shared);
  const adopted: string[] = [];
  withWindow(embedWindow, () => {
    const stop = requestSessionHandoff(token => adopted.push(token));
    for (const key of ['gah-theme', HANDOFF_KEY]) {
      const event = new Event('storage') as Event & { key?: string | null };
      event.key = key;
      embedWindow.dispatchEvent(event);
    }
    expect(adopted).toEqual([]);
    stop();
  });
});
