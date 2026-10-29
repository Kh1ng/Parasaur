import { webSocketUrl } from '../ws/webSocketUrl.js';
import { gahApi, pairingApi, diagnosticsApi, loginRepairApi, activityApi } from './client.js';

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
function captureFetch(): { requested: () => URL; requests: { url: string; init?: RequestInit }[]; restore: () => void } {
  const requests: { url: string; init?: RequestInit }[] = [];
  const original = globalThis.fetch;
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    requests.push({ url: String(input), init });
    return new Response(JSON.stringify({
      context: { project: 'group/repo', kind: 'issue', iid: 42, webUrl: null, profiles: [] },
      runs: [],
      events: []
    }), { status: String(input).endsWith('/update') ? 202 : 200 });
  }) as unknown as typeof fetch;
  return {
    requested: () => new URL(requests.at(-1)?.url ?? ''),
    requests,
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

// Exercise the real helpers, including the direct fetches with special response
// handling. Root deployments must retain the exact original URL bytes.
test.each(['', '/parasaur'])('all REST methods and direct fetches retain prefix %s', async prefix => {
  await withInjectedBootstrap({ basePath: prefix }, async () => {
    const captured = captureFetch();
    try {
      await gahApi.getStatus('a/b');
      await pairingApi.logout();
      await gahApi.saveNotificationSettings({} as Parameters<typeof gahApi.saveNotificationSettings>[0]);
      await pairingApi.setApprover('a/b', true);
      await pairingApi.revoke('a/b');
      await gahApi.startLoop('main');
      await gahApi.stopLoop('main');
      await gahApi.startAdminUpdate();
      await gahApi.startNodeUpdate('a/b');
      await diagnosticsApi.exportBundle();
      await loginRepairApi.submit('a/b', 'repair-key', 'input');
      await activityApi.notifications();
      expect(captured.requests.map(({ url, init }) => [url, init?.method ?? 'GET'])).toEqual([
        [`http://localhost:3000${prefix}/api/status?profile=a%2Fb`, 'GET'],
        [`http://localhost:3000${prefix}/api/pairing/logout`, 'POST'],
        [`http://localhost:3000${prefix}/api/settings/notifications`, 'PUT'],
        [`http://localhost:3000${prefix}/api/pairing/devices/a%2Fb`, 'PATCH'],
        [`http://localhost:3000${prefix}/api/pairing/devices/a%2Fb`, 'DELETE'],
        [`http://localhost:3000${prefix}/api/loop/start`, 'POST'],
        [`http://localhost:3000${prefix}/api/loop/stop`, 'POST'],
        [`http://localhost:3000${prefix}/api/admin/update`, 'POST'],
        [`http://localhost:3000${prefix}/api/registry/nodes/a%2Fb/update`, 'POST'],
        [`http://localhost:3000${prefix}/api/diagnostics/export`, 'GET'],
        [`http://localhost:3000${prefix}/api/auth-health/repairs/a%2Fb/input`, 'POST'],
        [`http://localhost:3000${prefix}/api/activity/notifications`, 'GET']
      ]);
      expect(captured.requests[1].init?.headers).toEqual(expect.objectContaining({ 'Idempotency-Key': expect.any(String) }));
      expect(captured.requests[10].init?.headers).toEqual(expect.objectContaining({ 'X-Login-Repair-Key': 'repair-key' }));
    } finally {
      captured.restore();
    }
  });
});


test('a prefixed SERVER_URL and injected bootstrap preserve the remote deployment', async () => {
  const previous = process.env.VITE_SERVER_URL;
  const SERVER_URL = 'https://remote.example/parasaur/';
  process.env.VITE_SERVER_URL = SERVER_URL;
  const captured = captureFetch();
  try {
    await withInjectedBootstrap({ basePath: new URL(SERVER_URL).pathname.replace(/\/$/, '') }, async () => {
      let remoteApi: typeof gahApi;
      jest.isolateModules(() => { remoteApi = require('./client.js').gahApi; });
      await remoteApi!.getStatus();
      expect(captured.requested().href).toBe(`${SERVER_URL}api/status`);
      await remoteApi!.sendTestNotification();
      expect(captured.requested().href).toBe(`${SERVER_URL}api/settings/notifications/test`);
      expect(webSocketUrl(new URL(SERVER_URL), undefined, 'main'))
        .toBe('wss://remote.example/parasaur/ws?profile=main');
    });
  } finally {
    captured.restore();
    if (previous === undefined) delete process.env.VITE_SERVER_URL;
    else process.env.VITE_SERVER_URL = previous;
  }
});
