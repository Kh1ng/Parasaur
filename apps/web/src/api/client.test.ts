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

test.each(['status', 'decision'] as const)('factory %s reads time out a silent response body and permit a subsequent read', async kind => {
  const original = globalThis.fetch;
  jest.useFakeTimers();
  const read = () => kind === 'status' ? gahApi.getLoopStatus('work') : gahApi.getLoopDecision('work');
  globalThis.fetch = (async (_input, init) => ({
    ok: true,
    json: () => new Promise((_resolve, reject) => {
      init?.signal?.addEventListener('abort', () => reject(new Error('read aborted')), { once: true });
    })
  })) as typeof fetch;
  try {
    await withInjectedBootstrap({ basePath: '/parasaur' }, async () => {
      const pending = read().catch(error => error);
      await jest.advanceTimersByTimeAsync(15_000);
      expect(await pending).toEqual(new Error('read aborted'));
      expect(jest.getTimerCount()).toBe(0);
      globalThis.fetch = (async () => new Response('null')) as typeof fetch;
      expect(await read()).toBeNull();
      expect(jest.getTimerCount()).toBe(0);
    });
  } finally {
    globalThis.fetch = original;
    jest.useRealTimers();
  }
});

test('model and account discovery forward cancellation without losing node, instance or deployment identity', async () => {
  const captured = captureFetch();
  const controller = new AbortController();
  try {
    await withInjectedBootstrap({ basePath: '/parasaur' }, async () => {
      await gahApi.getManagerChatModelsForBackend('work', 'codex', 'worker-one', 'work-account', controller.signal);
      expect(captured.requested().pathname).toBe('/parasaur/api/manager-chat/models');
      expect(captured.requested().searchParams.get('profile')).toBe('work');
      expect(captured.requested().searchParams.get('nodeId')).toBe('worker-one');
      expect(captured.requested().searchParams.get('backendInstance')).toBe('work-account');
      expect(captured.requests[0].init?.signal).toBe(controller.signal);
      await gahApi.getChatAccounts('work', 'codex', 'worker-two', controller.signal);
      expect(captured.requested().pathname).toBe('/parasaur/api/manager-chat/accounts');
      expect(captured.requested().searchParams.get('profile')).toBe('work');
      expect(captured.requested().searchParams.get('backend')).toBe('codex');
      expect(captured.requested().searchParams.get('nodeId')).toBe('worker-two');
      expect(captured.requests[1].init?.signal).toBe(controller.signal);
    });
  } finally { captured.restore(); }
});

test('work history stays project-scoped even when an older server returns every project', async () => {
  const original = globalThis.fetch;
  let requested = '';
  globalThis.fetch = (async input => {
    requested = String(input);
    return new Response(JSON.stringify([{ profile: 'alpha' }, { profile: 'beta' }]));
  }) as typeof fetch;
  try {
    expect(await gahApi.getWorkTimeline('#42', 'beta')).toEqual([{ profile: 'beta' }]);
    expect(new URL(requested).searchParams.get('profile')).toBe('beta');
    expect(await gahApi.getWorkTimeline('#42')).toEqual([{ profile: 'alpha' }, { profile: 'beta' }]);
  } finally {
    globalThis.fetch = original;
  }
});

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
