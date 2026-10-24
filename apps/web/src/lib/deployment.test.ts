import { endpointUrl } from './deployment.js';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { runInNewContext } from 'node:vm';

test('REST paths retain the deployment prefix and work at the origin root', () => {
  expect(endpointUrl('/api/profiles', 'https://gitlab.example/parasaur').href).toBe('https://gitlab.example/parasaur/api/profiles');
  expect(endpointUrl('/api/profiles', 'https://gitlab.example/parasaur/').href).toBe('https://gitlab.example/parasaur/api/profiles');
  expect(endpointUrl('/api/profiles', 'https://parasaur.example').href).toBe('https://parasaur.example/api/profiles');
});

test('the service worker scopes its shell and leaves prefixed APIs and embed policy uncached', async () => {
  const listeners = new Map<string, (event: unknown) => void>();
  const cached: string[][] = [];
  const removed: string[] = [];
  runInNewContext(readFileSync(join(__dirname, '../../public/sw.js'), 'utf8'), {
    URL,
    self: {
      registration: { scope: 'https://gitlab.example/parasaur/' },
      location: { origin: 'https://gitlab.example' },
      addEventListener: (name: string, callback: (event: unknown) => void) => listeners.set(name, callback),
      skipWaiting: () => Promise.resolve(),
      clients: { claim: () => Promise.resolve() }
    },
    caches: {
      open: async () => ({ addAll: async (assets: string[]) => { cached.push(assets); } }),
      keys: async () => ['gah-shell:/parasaur/:v1', 'gah-shell:/parasaur/:v2', 'gah-shell:/:v2'],
      delete: async (key: string) => { removed.push(key); }
    }
  });
  let installed: Promise<void> | undefined;
  listeners.get('install')!({ waitUntil: (promise: Promise<void>) => { installed = promise; } });
  await installed;
  expect(cached[0]).toContain('/parasaur/');
  let handled = false;
  for (const path of ['/parasaur/api/status', '/parasaur/embed', '/api/status']) {
    listeners.get('fetch')!({ request: { url: `https://gitlab.example${path}`, method: 'GET', mode: 'navigate' }, respondWith: () => { handled = true; } });
  }
  expect(handled).toBe(false);
  let activated: Promise<void> | undefined;
  listeners.get('activate')!({ waitUntil: (promise: Promise<void>) => { activated = promise; } });
  await activated;
  expect(removed).toEqual(['gah-shell:/parasaur/:v1']);
});
