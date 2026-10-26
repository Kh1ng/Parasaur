import { basePath, readBootstrap, withBasePath } from './basePath.js';

/** The bootstrap is injected into a real page; tests stand it up on globalThis. */
function withInjectedBootstrap(value: unknown, run: () => void): void {
  const previous = (globalThis as Record<string, unknown>).window;
  (globalThis as Record<string, unknown>).window = value === undefined ? {} : { __GAH_BOOTSTRAP__: value };
  try {
    run();
  } finally {
    if (previous === undefined) delete (globalThis as Record<string, unknown>).window;
    else (globalThis as Record<string, unknown>).window = previous;
  }
}

test('a missing or malformed injection falls back to the root deployment', () => {
  withInjectedBootstrap(undefined, () => {
    expect(basePath()).toBe('');
    expect(withBasePath('/api/status')).toBe('/api/status');
  });
  for (const malformed of [null, '', '/parasaur', 42, { basePath: 7 }, { basePath: '//evil.example' }, { basePath: '/..' }, { basePath: '/parasaur/../x' }, { basePath: `/${'a'.repeat(129)}` }, { basePath: '/parasaur/' }]) {
    withInjectedBootstrap(malformed, () => {
      expect(readBootstrap().basePath).toBe('');
    });
  }
});

test('a valid prefix is read and joined into app-absolute paths', () => {
  withInjectedBootstrap({ basePath: '/parasaur', gitLabOrigins: ['https://gitlab.example.com'] }, () => {
    expect(basePath()).toBe('/parasaur');
    expect(withBasePath('/api/status')).toBe('/parasaur/api/status');
    expect(withBasePath('/ws')).toBe('/parasaur/ws');
    expect(withBasePath('/sw.js')).toBe('/parasaur/sw.js');
    // Relative and already-prefixed paths pass through untouched.
    expect(withBasePath('assets/index.js')).toBe('assets/index.js');
    expect(withBasePath('/parasaur/api/x')).toBe('/parasaur/parasaur/api/x');
  });
});

test('gitLab origins are validated exactly like the server injected them', () => {
  withInjectedBootstrap({ gitLabOrigins: [
    'https://gitlab.example.com', 'https://gitlab.example.com', 'https://two.example',
    'http://insecure.example:8080', 'https://with-path.example.com/x', 'ftp://other.example', 'not a url', 7, null
  ] }, () => {
    expect(readBootstrap().gitLabOrigins).toEqual(['https://gitlab.example.com', 'https://two.example', 'http://insecure.example:8080']);
  });
  withInjectedBootstrap({ gitLabOrigins: 'https://gitlab.example.com' }, () => {
    expect(readBootstrap().gitLabOrigins).toEqual([]);
  });
});
