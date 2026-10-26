import http from 'node:http';
import express from 'express';
import { sessionHandoffRouter } from '../../../server/src/sessionHandoff.js';
import { authMiddleware } from '../../../server/src/authMiddleware.js';
import { expect, test, type APIRequestContext } from '@playwright/test';

/**
 * GitLab panel embed view (#1593). The mock control plane always authorizes
 * (`/api/pairing/session` returns its owner principal), so these cover the
 * chrome-less route, the standalone URL with app chrome, and the panel
 * postMessage handshake; the signed-out gate rides the same 401 the rest of
 * the dashboard shows.
 */

const issueContext = '/embed?gitlab=https%3A%2F%2Fgitlab.example.com&url=https%3A%2F%2Fgitlab.example.com%2Fteam%2Fproject&project=team%2Fproject&kind=issue&iid=42';

const mockUrl = () => process.env.GAH_MOCK_BASE_URL ?? 'http://127.0.0.1:3774';

/** Stand the mock's project list up the way a real GitLab install has it:
 * two uniquely matching GitLab profiles for the two projects the panel
 * pages below name. */
async function addGitLabProfiles(request: APIRequestContext): Promise<void> {
  await request.post(`${mockUrl()}/api/mock/reset`);
  for (const [name, repo] of [['gitlab-project', 'team/project'], ['gitlab-other', 'team/other']] as const) {
    const response = await request.post(`${mockUrl()}/api/profiles`, {
      data: { name, display_name: name, repo_id: name, provider: 'gitlab', repo, local_path: `/mock/${name}`, artifact_root: `/mock/${name}` }
    });
    expect(response.ok()).toBeTruthy();
  }
}

// The mock control plane is shared with every other spec file; hand it back
// with its default single-project state.
test.afterAll(async ({ request }: { request: APIRequestContext }) => {
  await request.post(`${mockUrl()}/api/mock/reset`);
});

test('the embed route renders the context surface chrome-less', async ({ page }) => {
  await page.goto(issueContext);
  await expect(page.getByRole('heading', { name: 'team/project · Issue · #42' })).toBeVisible();
  // Chrome-less: the app shell's sidebar stays absent, the pop-out stays.
  await expect(page.getByRole('navigation', { name: 'Sidebar' })).toHaveCount(0);
  await expect(page.getByRole('link', { name: 'Open in Parasaur' })).toHaveCount(1);
  // The mock's only project is a github fixture, so no Parasaur project
  // matches the GitLab page: the guidance shows and actions stay hidden.
  await expect(page.getByText('No Parasaur project matches team/project')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Work on this issue' })).toHaveCount(0);
  // The agent chat with its model and skills pickers still renders.
  await expect(page.getByRole('button', { name: 'New chat' })).toBeVisible();
});

test('the panel handshake updates the context from a configured origin', async ({ page }) => {
  // The dev server has no server-side injection; stand the configured
  // origin up the way the served document receives it, then speak the
  // panel's protocol from that origin.
  await page.addInitScript(() => {
    window.__GAH_BOOTSTRAP__ = { basePath: '', gitLabOrigins: [window.location.origin] };
  });
  await page.goto('/embed');
  await expect(page.getByText('Waiting for the GitLab page context')).toBeVisible();
  await page.evaluate(() => {
    window.postMessage({ type: 'parasaur:context', context: {
      gitlab: 'https://gitlab.example.com',
      url: 'https://gitlab.example.com/team/project/-/merge_requests/42',
      project: 'team/project', kind: 'merge_request', iid: 42
    } }, '*');
  });
  await expect(page.getByRole('heading', { name: 'team/project · Merge request · #42' })).toBeVisible();
});

test('the standalone URL opens the same context view with app chrome', async ({ page }) => {
  await page.goto('/?gitlab=https%3A%2F%2Fgitlab.example.com&url=https%3A%2F%2Fgitlab.example.com%2Fteam%2Fproject&project=team%2Fproject&kind=merge_request&iid=42');
  await expect(page.getByRole('heading', { name: 'team/project · Merge request · #42' })).toBeVisible();
  // App chrome present, the context parameters left the URL, and leaving the
  // context view restores the normal dashboard page.
  await expect(page.getByRole('navigation', { name: 'Sidebar' })).toBeVisible();
  expect(new URL(page.url()).searchParams.get('gitlab')).toBeNull();
  await page.getByRole('button', { name: 'Leave the GitLab context view' }).click();
  await expect(page.getByRole('heading', { name: 'Overview' })).toBeVisible();
});

test('ordinary chat binds to the matched GitLab project and carries the page context', async ({ page, request }) => {
  await addGitLabProfiles(request);
  await page.goto(issueContext);
  await expect(page.getByRole('heading', { name: 'team/project · Issue · #42' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Work on this issue' })).toBeVisible();
  // The chat the embed view starts is bound to the matched project — not to
  // whatever the dashboard selected last — and an ordinary first turn
  // carries the resolved page context with it.
  const projectSelect = page.getByRole('combobox', { name: 'Project' });
  await expect(projectSelect).toHaveValue('gitlab-project');
  const composer = page.getByPlaceholder(/Message the manager/);
  await composer.fill('Where should the fix land?');
  await composer.press('Enter');
  await expect(page.getByText('GitLab page context (from the Parasaur panel):')).toBeVisible();
  await expect(page.getByText(/Project: team\/project/)).toBeVisible();
  await expect(page.getByText(/Issue: #42/)).toBeVisible();
  await expect(page.getByText(/Page: https:\/\/gitlab.example.com\/team\/project/)).toBeVisible();
  await expect(page.getByText('Where should the fix land?')).toBeVisible();
});

test('a context update re-binds the chat to the newly matched project', async ({ page, request }) => {
  await addGitLabProfiles(request);
  await page.addInitScript(() => {
    window.__GAH_BOOTSTRAP__ = { basePath: '', gitLabOrigins: [window.location.origin] };
  });
  await page.goto(issueContext);
  const projectSelect = page.getByRole('combobox', { name: 'Project' });
  await expect(projectSelect).toHaveValue('gitlab-project');
  // The panel reports a different GitLab page: the match moves, and the
  // ordinary chat must follow it instead of keeping the earlier selection.
  await page.evaluate(() => {
    window.postMessage({ type: 'parasaur:context', context: {
      gitlab: 'https://gitlab.example.com',
      url: 'https://gitlab.example.com/team/other/-/issues/7',
      project: 'team/other', kind: 'issue', iid: 7
    } }, '*');
  });
  await expect(page.getByRole('heading', { name: 'team/other · Issue · #7' })).toBeVisible();
  await expect(projectSelect).toHaveValue('gitlab-other');
});

test('an owner signs in through the real handoff router and the iframe keeps its token tab-scoped', async ({ page, context }) => {
  const previous = process.env.COORDINATOR_TOKEN;
  process.env.COORDINATOR_TOKEN = 'owner-handoff-e2e-fixture';
  const app = express();
  app.set('trust proxy', 'loopback');
  app.use(express.json());
  app.use('/api/pairing/handoff', sessionHandoffRouter());
  app.get('/api/pairing/session', authMiddleware, (_req, res) => res.json({ principal: res.locals.authPrincipal }));
  const server = http.createServer(app);
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('Missing test address');
  const base = `http://127.0.0.1:${address.port}`;
  // Forward browser requests to the production router, simulating TLS proxy
  // headers. The rest of the dashboard still uses the hermetic mock server.
  await context.route(/\/api\/pairing\/(handoff(?:\/[^?]*)?|session)(?:\?.*)?$/, async route => {
    const request = route.request();
    const response = await fetch(base + new URL(request.url()).pathname, {
      method: request.method(),
      headers: { 'Content-Type': 'application/json', Origin: base.replace('http:', 'https:'),
        'X-Forwarded-For': '198.51.100.2', 'X-Forwarded-Proto': 'https',
        ...(request.headers().authorization ? { Authorization: request.headers().authorization } : {}) },
      ...(request.postData() ? { body: request.postData()! } : {})
    });
    await route.fulfill({ status: response.status, contentType: 'application/json', body: await response.text() });
  });
  try {
    await page.route('**/embed-parent', route => route.fulfill({ contentType: 'text/html', body: `<iframe title="panel" src="${issueContext}"></iframe>` }));
    await page.goto('/embed-parent');
    const panel = page.frameLocator('iframe');
    await expect(panel.getByRole('dialog', { name: 'Sign in to Parasaur' })).toBeVisible();
    const link = panel.getByRole('link', { name: 'Open Parasaur to sign in' });
    await expect(link).toHaveAttribute('href', /#gah_signin=/);
    const popup = context.waitForEvent('page');
    await link.click();
    const signInTab = await popup;
    await signInTab.waitForLoadState();
    await expect(signInTab.getByRole('button', { name: 'Connect GitLab panel' })).toBeVisible();
    expect(new URL(signInTab.url()).hash).toBe('');
    await signInTab.getByText('Central access token', { exact: true }).click();
    await signInTab.getByLabel('Access token', { exact: true }).fill('owner-handoff-e2e-fixture');
    await signInTab.getByRole('button', { name: 'Save and reconnect' }).click();
    await signInTab.getByRole('button', { name: 'Connect GitLab panel' }).click();
    await expect(panel.getByRole('dialog', { name: 'Sign in to Parasaur' })).toHaveCount(0, { timeout: 8_000 });
    const frame = page.frames().find(candidate => candidate.url().includes('/embed?'))!;
    expect(await frame.evaluate(() => sessionStorage.getItem('gah.coordinatorToken'))).toBe('owner-handoff-e2e-fixture');
    // A same-origin parent shares its iframe's tab-scoped storage.
    expect(await page.evaluate(() => sessionStorage.getItem('gah.coordinatorToken'))).toBe('owner-handoff-e2e-fixture');
    await signInTab.close();
    expect(await page.evaluate(() => Object.keys(localStorage).filter(key => key.startsWith('gah.coordinatorToken')))).toEqual([]);
    const otherTab = await context.newPage();
    await otherTab.goto('/');
    expect(await otherTab.evaluate(() => sessionStorage.getItem('gah.coordinatorToken'))).toBeNull();
    await otherTab.close();
  } finally {
    await new Promise<void>(resolve => server.close(() => resolve()));
    if (previous === undefined) delete process.env.COORDINATOR_TOKEN; else process.env.COORDINATOR_TOKEN = previous;
  }
});

test('the signed-out embed view keeps re-checking the auth endpoint and picks up a later session', async ({ page }) => {
  // Fake auth endpoint that starts refusing (no session in this browsing
  // context) and accepts once the sign-in lands — e.g. a cookie-based
  // pairing session the iframe's requests already carry.
  let signedIn = false;
  await page.route('**/api/pairing/session', route => {
    if (signedIn) return route.fulfill({ json: { principal: { kind: 'owner' }, can_approve_pairing: true } });
    return route.fulfill({ status: 401, json: { message: 'Sign in required' } });
  });
  await page.goto(issueContext);
  await expect(page.getByRole('dialog', { name: 'Sign in to Parasaur' })).toBeVisible();
  signedIn = true;
  await expect(page.getByRole('dialog', { name: 'Sign in to Parasaur' })).toHaveCount(0, { timeout: 8_000 });
});
