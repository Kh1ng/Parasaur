import { expect, test, type WebSocketRoute } from '@playwright/test';

test('account activity requires local exact execution evidence rather than a claim or transport', async ({ page }) => {
  let socket: WebSocketRoute | undefined;
  await page.routeWebSocket('**/ws**', ws => {
    socket = ws;
    ws.send(JSON.stringify({ type: 'server.welcome', serverVersion: 'fixture', profile: 'fixture', serverProviderCatalog: { providers: [] }, providers: {}, sessions: [] }));
    ws.send(JSON.stringify({ type: 'activity.replay', events: [] }));
  });
  await page.route('**/api/status**', async route => {
    const snapshot = await (await route.fetch()).json();
    snapshot.node_id = 'local';
    snapshot.running_workers = [];
    snapshot.active_claims = [{ work_id: '#42', pid: 42, scope: 'fixture', hostname: 'fixture', claimed_at: new Date().toISOString(), age_seconds: 0, route: { backend: 'codex', backend_instance: 'work' } }];
    snapshot.recent_ledger = { ...snapshot.recent_ledger, most_recent_work_id: '#42', most_recent_effective_backend: 'codex' };
    await route.fulfill({ json: snapshot });
  });
  await page.route('**/api/quota?*', async route => {
    const snapshot = await (await route.fetch()).json();
    snapshot.quota_checks = [];
    snapshot.account_spend = [];
    snapshot.candidates = ['personal', 'work'].map(instance => ({ backend: 'codex', backend_instance: instance, provider: 'openai', model: 'same-model', modes: ['improve'], configured: true, eligible_now: true, usage: snapshot.usage }));
    await route.fulfill({ json: snapshot });
  });
  try {
    await page.goto('/?page=quota&profile=fixture');
    const chips = page.getByRole('group', { name: 'Subscription usage' });
    const work = chips.getByRole('button', { name: /^OpenAI work usage/ });
    const personal = chips.getByRole('button', { name: /^OpenAI personal usage/ });
    await expect(work).toBeVisible();
    await expect(personal).toBeVisible();
    await expect(chips.locator('[data-working]')).toHaveCount(0);
    await expect.poll(() => Boolean(socket)).toBe(true);
    const session = { id: 'activity-session', providerKind: 'github', instanceId: 'personal', backend: 'codex', backendInstance: 'work', nodeId: 'local', status: 'starting' };
    const send = (value: Record<string, unknown>) => socket!.send(JSON.stringify(value));
    send({ type: 'session.started', session });
    await expect(chips.locator('[data-working]')).toHaveCount(0);
    send({ type: 'session.status', session: { ...session, status: 'running' } });
    await expect(work.locator('[data-working]')).toHaveCount(1);
    await expect(personal.locator('[data-working]')).toHaveCount(0);
    send({ type: 'session.status', session: { ...session, status: 'running', nodeId: 'remote' } });
    await expect(chips.locator('[data-working]')).toHaveCount(0);
    const worker = { profile: 'fixture', work_id: '#42', run_id: 'runtime', mode: 'improve', backend: 'codex', runner: 'codex', backend_instance: 'work', requested_model: 'same-model', model: 'same-model', actual_model: null, node_id: 'local', branch: 'gah/42', started_at: new Date().toISOString(), last_activity_at: new Date().toISOString(), attempt: 1, stale_after_seconds: 30, state: 'running' };
    send({ type: 'workers.snapshot', profile: 'fixture', workers: [worker] });
    await expect(work.locator('[data-working]')).toHaveCount(1);
    send({ type: 'workers.snapshot', profile: 'fixture', workers: [{ ...worker, state: 'stale' }] });
    await expect(chips.locator('[data-working]')).toHaveCount(0);
    send({ type: 'session.status', session: { ...session, status: 'running', backendInstance: undefined } });
    await expect(chips.locator('[data-working]')).toHaveCount(0);
  } finally { await page.unrouteAll({ behavior: 'ignoreErrors' }); }
});
