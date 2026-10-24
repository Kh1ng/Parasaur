import { expect, test } from '@playwright/test';

const origin = 'https://gitlab.example';
const context = { gitlab: origin, url: `${origin}/team/repo/-/merge_requests/12`, project: 'team/repo', kind: 'mr', iid: '12', ref: 'main', path: 'src/index.ts' };

test('embed scopes chat, uses the forwarded prefix, and validates parent messages', async ({ page }) => {
  const profile = { name: 'alpha', display_name: 'Alpha', repo: 'team/repo', provider: 'gitlab', web_url: `${origin}/team/repo` };
  const session = { id: 'context-chat', profile: 'alpha', title: 'MR 12', backend: 'codex', model: null, outcome: 'live', nodeId: 'central', branch: 'chat', worktreePath: '/tmp/chat', createdAt: Date.now(), lastActiveAt: Date.now(), archivedAt: null };
  const sent: Record<string, unknown>[] = [];
  const paths: string[] = [];
  await page.route('**/parasaur/embed?*', async route => {
    const response = await route.fetch();
    const html = (await response.text()).replace('<head>', `<head><script id="parasaur-config" type="application/json">${JSON.stringify({ prefix: '/parasaur', gitlabOrigins: [origin] })}</script>`);
    await route.fulfill({ response, body: html });
  });
  await page.route('**/api/**', async route => {
    const url = new URL(route.request().url());
    paths.push(url.pathname);
    const path = url.pathname.replace(/^\/parasaur/, '');
    if (path === '/api/profiles') return route.fulfill({ json: [profile] });
    if (path === '/api/projects') return route.fulfill({ json: [] });
    if (path === '/api/manager-chat/nodes') return route.fulfill({ json: { nodes: [{ nodeId: 'central', displayName: 'Coordinator', role: 'central', eligible: true, chatCapable: true, lastSeenAt: null }] } });
    if (path === '/api/manager-chat/sessions') return route.fulfill({ json: route.request().method() === 'POST' ? session : { sessions: [session] } });
    if (path === '/api/manager-chat/settings') return route.fulfill({ json: { defaultBackend: 'codex', profileOverrides: {}, availableBackends: [{ id: 'codex', displayName: 'Codex', implemented: true }] } });
    if (path === '/api/manager-chat/models') return route.fulfill({ json: { models: [], currentModelId: null } });
    if (path === '/api/manager-chat/commands') return route.fulfill({ json: { commands: [] } });
    return route.fulfill({ status: 503, json: { error: 'Unavailable in embed test' } });
  });
  let wsPath = '';
  await page.routeWebSocket('**/ws**', ws => {
    wsPath = new URL(ws.url()).pathname;
    ws.send(JSON.stringify({ type: 'server.welcome', serverVersion: 'test', serverProviderCatalog: { providers: [] }, sessions: [
      { id: 'matching-run', repo: 'team/repo', mr: '12', status: 'running' },
      { id: 'other-run', repo: 'team/other', target: '12', status: 'running' }
    ], providers: {}, profile: 'unrelated' }));
    ws.onMessage(raw => {
      const message = JSON.parse(String(raw));
      if (message.type === 'manager.chat.send') sent.push(message);
      if (message.type === 'manager.chat.historyRequest') ws.send(JSON.stringify({ type: 'manager.chat.history', requestId: message.requestId, profile: message.profile, turns: [], cursor: 0, streaming: null }));
    });
  });
  await page.goto(`/parasaur/embed?${new URLSearchParams(context)}`);
  await expect(page.getByRole('button', { name: 'Review this MR', exact: true })).toBeVisible();
  await expect(page.getByRole('navigation', { name: 'Main navigation' })).toHaveCount(0);
  await page.getByText('Runs for this context (1)').click();
  await expect(page.getByText('matching-run · running')).toBeVisible();
  await expect(page.getByText('other-run · running')).toHaveCount(0);
  await page.getByRole('button', { name: 'Review this MR', exact: true }).click();
  await expect(page.getByPlaceholder(/Message the manager/)).toHaveValue('Review this MR');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect.poll(() => sent.length).toBe(1);
  expect(sent[0].profile).toBe('alpha');
  expect(sent[0].sessionId).toBe('context-chat');
  expect(sent[0].message).toContain(context.url);
  expect(sent[0].message).toContain('Review this MR');
  expect(paths.every(path => path.startsWith('/parasaur/api/'))).toBeTruthy();
  expect(wsPath).toBe('/parasaur/ws');
  await expect(page.getByRole('link', { name: 'Open in Parasaur' })).toHaveAttribute('href', /\/parasaur\/embed\?.*standalone=1/);
  await page.evaluate(({ context, origin }) => {
    window.dispatchEvent(new MessageEvent('message', { origin: 'https://evil.example', source: window, data: { type: 'parasaur:context', context: { ...context, kind: 'issue' } } }));
    window.dispatchEvent(new MessageEvent('message', { origin, source: null, data: { type: 'parasaur:context', context: { ...context, kind: 'issue' } } }));
  }, { context, origin });
  await expect(page.getByRole('button', { name: 'Review this MR', exact: true })).toBeVisible();
  await page.evaluate(({ context, origin }) => window.dispatchEvent(new MessageEvent('message', { origin, source: window, data: { type: 'parasaur:context', context: { ...context, kind: 'issue', iid: '13', url: `${origin}/team/repo/-/issues/13` } } })), { context, origin });
  await expect(page.getByRole('button', { name: 'Work on this issue', exact: true })).toBeVisible();
});
