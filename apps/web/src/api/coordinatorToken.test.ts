import { coordinatorToken, saveCoordinatorToken } from './coordinatorToken.js';

/** Keep token saves tab-scoped even if another tab sets the old marker. */
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

const WANTED_KEY = 'gah.coordinatorTokenWanted';

test('saving and clearing an owner token never accesses localStorage, even when another tab requests it', () => {
  const shared = new FakeStorage();
  shared.setItem(WANTED_KEY, '1');
  const read = jest.spyOn(shared, 'getItem');
  const write = jest.spyOn(shared, 'setItem');
  const remove = jest.spyOn(shared, 'removeItem');
  withWindow(new FakeWindow(shared), () => {
    saveCoordinatorToken('owner-token');
    expect(coordinatorToken()).toBe('owner-token');
    saveCoordinatorToken('');
    expect(coordinatorToken()).toBe('');
  });
  expect(read).not.toHaveBeenCalled();
  expect(write).not.toHaveBeenCalled();
  expect(remove).not.toHaveBeenCalled();
});
