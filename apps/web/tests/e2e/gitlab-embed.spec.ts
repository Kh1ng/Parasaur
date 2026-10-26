import { expect, test } from '@playwright/test';

/**
 * GitLab panel embed view (#1593). The mock control plane always authorizes
 * (`/api/pairing/session` returns its owner principal), so these cover the
 * chrome-less route, the standalone URL with app chrome, and the panel
 * postMessage handshake; the signed-out gate rides the same 401 the rest of
 * the dashboard shows.
 */

const issueContext = '/embed?gitlab=https%3A%2F%2Fgitlab.example.com&url=https%3A%2F%2Fgitlab.example.com%2Fteam%2Fproject&project=team%2Fproject&kind=issue&iid=42';

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
