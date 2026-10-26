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

test('a sign-in finished in another tab reaches the waiting iframe through the session handoff', async ({ page, context }) => {
  // Fake auth endpoint: only the credential the sign-in tab issued works.
  await page.route('**/api/pairing/session', route => {
    if (route.request().headers().authorization === 'Bearer token-from-sign-in-tab') {
      return route.fulfill({ json: { principal: { kind: 'owner' }, can_approve_pairing: true } });
    }
    return route.fulfill({ status: 401, json: { message: 'Sign in required' } });
  });
  await page.goto(issueContext);
  await expect(page.getByRole('dialog', { name: 'Sign in to Parasaur' })).toBeVisible();
  // The prompt's link opened a new tab; the owner signed in there. That
  // tab's sessionStorage is invisible here, so its save mirrors the
  // credential through the same-origin storage handoff — never through the
  // embedding page. A real second tab on the same origin fires the storage
  // event into this iframe.
  const signInTab = await context.newPage();
  await signInTab.goto('/');
  await signInTab.evaluate(() => localStorage.setItem('gah.coordinatorTokenHandoff', 'token-from-sign-in-tab'));
  await expect(page.getByRole('dialog', { name: 'Sign in to Parasaur' })).toHaveCount(0);
  expect(await page.evaluate(() => sessionStorage.getItem('gah.coordinatorToken'))).toBe('token-from-sign-in-tab');
  await signInTab.close();
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
