import { gahApi } from './client.js';

/** The deployment bootstrap is injected into a real page; tests stand it up
 * on globalThis the same way lib/basePath.test.ts does. */
async function withInjectedBootstrap(value: unknown, run: () => Promise<void> | void): Promise<void> {
  const global = globalThis as Record<string, unknown>;
  const previous = global.window;
  global.window = value === undefined ? {} : { __GAH_BOOTSTRAP__: value };
  try {
    await run();
  } finally {
    if (previous === undefined) delete global.window;
    else global.window = previous;
  }
}

/** Record every request URL the client issues and answer with an empty feed. */
function captureFetch(): { requested: () => URL; restore: () => void } {
  const urls: string[] = [];
  const original = globalThis.fetch;
  globalThis.fetch = (async (input: RequestInfo | URL) => {
    urls.push(String(input));
    return new Response(JSON.stringify({
      context: { project: 'group/repo', kind: 'issue', iid: 42, webUrl: null, profiles: [] },
      runs: [],
      events: []
    }));
  }) as unknown as typeof fetch;
  return {
    requested: () => new URL(urls.at(-1) ?? ''),
    restore: () => {
      const global = globalThis as Record<string, unknown>;
      if (original === undefined) delete global.fetch;
      else global.fetch = original;
    }
  };
}

const CONTEXT = { project: 'group/repo', kind: 'issue' as const, iid: 42, gitlab: 'https://gitlab.example.com', branch: 'main', limit: 10 };

test('getGitLabRuns keeps the deployment prefix in the request URL', async () => {
  await withInjectedBootstrap({ basePath: '/parasaur', gitLabOrigins: [] }, async () => {
    const fetch = captureFetch();
    try {
      await gahApi.getGitLabRuns(CONTEXT);
      const requested = fetch.requested();
      expect(requested.origin).toBe('http://localhost:3000');
      expect(requested.pathname).toBe('/parasaur/api/integrations/gitlab/runs');
      expect(requested.searchParams.get('project')).toBe('group/repo');
      expect(requested.searchParams.get('kind')).toBe('issue');
      expect(requested.searchParams.get('iid')).toBe('42');
      expect(requested.searchParams.get('gitlab')).toBe('https://gitlab.example.com');
      expect(requested.searchParams.get('branch')).toBe('main');
      expect(requested.searchParams.get('limit')).toBe('10');
    } finally {
      fetch.restore();
    }
  });
});

test('without a prefix the runs request stays at the root', async () => {
  await withInjectedBootstrap(undefined, async () => {
    const fetch = captureFetch();
    try {
      await gahApi.getGitLabRuns(CONTEXT);
      expect(fetch.requested().pathname).toBe('/api/integrations/gitlab/runs');
    } finally {
      fetch.restore();
    }
  });
});
