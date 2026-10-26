import { expect, test } from '@playwright/experimental-ct-react';
import type { GitLabRunsFeedResponse } from '@git-agent-harness/contracts';
import { GitLabRunsFeed } from '../../src/components/GitLabRunsFeed.js';

const context = {
  gitlab: 'https://gitlab.example.test',
  project: 'parasaur-test/group/project',
  kind: 'issue' as const,
  iid: 42
};

const feed: GitLabRunsFeedResponse = {
  context: {
    project: 'parasaur-test/group/project',
    kind: 'issue',
    iid: 42,
    webUrl: 'https://gitlab.example.test/parasaur-test/group/project',
    profiles: ['gl-profile']
  },
  runs: [
    {
      id: 'ledger:gl-profile:2026-10-22T10:00:00Z:improve',
      source: 'ledger',
      status: 'passed',
      mode: 'improve',
      workId: '#42',
      branch: 'gah/issue/project-42',
      backend: 'codex',
      model: 'gpt-6.1',
      startedAt: '2026-10-22T10:00:00Z',
      endedAt: '2026-10-22T10:05:00Z',
      durationSeconds: 300,
      testStatus: 'passed',
      mrUrl: 'https://gitlab.example.test/parasaur-test/group/project/-/merge_requests/7',
      title: null,
      profile: 'gl-profile',
      attempts: [
        { attempt: 1, backend: 'codex', model: 'gpt-6.1', exitCode: 1, validationResult: 'failed', failureClass: 'test_failure', durationSeconds: 60 },
        { attempt: 2, backend: 'codex', model: 'gpt-6.1', exitCode: 0, validationResult: 'passed', failureClass: null, durationSeconds: 240 }
      ]
    },
    {
      id: 'session-9',
      source: 'session',
      status: 'running',
      mode: 'review',
      workId: null,
      branch: null,
      backend: 'vibe',
      model: 'mistral-medium-3.5',
      startedAt: '2026-10-25T08:00:00Z',
      endedAt: null,
      durationSeconds: null,
      testStatus: null,
      mrUrl: null,
      title: null,
      profile: 'gl-profile',
      attempts: []
    }
  ],
  events: []
};

/** The feed's only data source: the runs endpoint, stubbed per test. */
async function stubRuns(page: import('@playwright/test').Page, body: GitLabRunsFeedResponse) {
  await page.route('**/api/integrations/gitlab/runs**', route => route.fulfill({ json: body }));
}

test('renders the context runs with status, time, test status and links', async ({ mount, page }) => {
  await stubRuns(page, feed);
  const component = await mount(
    <GitLabRunsFeed context={context} panelUrl="https://parasaur.example.test/" />
  );

  await expect(component.getByTestId('gitlab-runs-list').locator('li')).toHaveCount(2);
  await expect(component.getByText('improve', { exact: true })).toBeVisible();
  await expect(component.getByText('Tests: passed', { exact: true })).toBeVisible();
  await expect(component.getByText('300s', { exact: true })).toBeVisible();
  await expect(component.getByText('2 attempts', { exact: true })).toBeVisible();
  await expect(component.getByText('#42', { exact: true })).toBeVisible();
  await expect(component.getByRole('link', { name: 'MR' })).toHaveAttribute(
    'href',
    'https://gitlab.example.test/parasaur-test/group/project/-/merge_requests/7'
  );
  // Live session runs deep-link the dashboard session modal (#1643's ?session= handling).
  const details = component.getByRole('link', { name: /Details/ }).first();
  await expect(details).toHaveAttribute('href', /page=work/);
  await expect(component.getByText('Started')).toHaveCount(2);
});

test('routes the query through the feed endpoint with the page context', async ({ mount, page }) => {
  const requests: string[] = [];
  page.on('request', request => { if (request.url().includes('/api/integrations/gitlab/runs')) requests.push(request.url()); });
  await stubRuns(page, feed);
  const component = await mount(
    <GitLabRunsFeed context={{ ...context, kind: 'merge_request', iid: 7, ref: 'gah/issue/project-42' }} limit={5} panelUrl="https://parasaur.example.test/" />
  );
  await expect(component.getByTestId('gitlab-runs-list').locator('li')).toHaveCount(2);
  expect(requests.length).toBeGreaterThan(0);
  const query = new URL(requests[requests.length - 1]).searchParams;
  expect(query.get('project')).toBe('parasaur-test/group/project');
  expect(query.get('kind')).toBe('merge_request');
  expect(query.get('iid')).toBe('7');
  expect(query.get('gitlab')).toBe('https://gitlab.example.test');
  expect(query.get('branch')).toBe('gah/issue/project-42');
  expect(query.get('limit')).toBe('5');
});

test('shows the empty state when the context has no runs yet', async ({ mount, page }) => {
  await stubRuns(page, { ...feed, runs: [] });
  const component = await mount(
    <GitLabRunsFeed context={context} panelUrl="https://parasaur.example.test/" />
  );
  await expect(component.getByText('No runs for this context yet')).toBeVisible();
  await expect(component.getByText('Runs Parasaur starts for this issue or merge request appear here.')).toBeVisible();
});

test('renders nothing for page kinds that have no runs', async ({ mount, page }) => {
  await stubRuns(page, feed);
  const component = await mount(
    <GitLabRunsFeed context={{ ...context, kind: 'file', iid: null }} panelUrl="https://parasaur.example.test/" />
  );
  await expect(component.getByTestId('gitlab-runs-list')).toHaveCount(0);
});

test('a stale response from the previous context never overwrites the current runs', async ({ mount, page }) => {
  // The issue-42 request hangs until the test resolves it, so its response
  // settles after the component has already moved to issue 43.
  let resolveIssue42!: (body: GitLabRunsFeedResponse) => void;
  const issue42 = new Promise<GitLabRunsFeedResponse>(resolve => { resolveIssue42 = resolve; });
  const issue43: GitLabRunsFeedResponse = {
    ...feed,
    context: { ...feed.context, iid: 43 },
    runs: feed.runs.map(run => ({ ...run, workId: '#43' }))
  };
  await page.route('**/api/integrations/gitlab/runs**', async route => {
    const iid = new URL(route.request().url()).searchParams.get('iid');
    if (iid === '42') {
      const held = await issue42;
      await route.fulfill({ json: held });
    } else {
      await route.fulfill({ json: issue43 });
    }
  });

  const component = await mount(
    <GitLabRunsFeed context={{ ...context, iid: 42 }} panelUrl="https://parasaur.example.test/" />
  );
  // Move to issue 43 while issue 42's response is still in flight; 43 loads.
  await component.update(
    <GitLabRunsFeed context={{ ...context, iid: 43 }} panelUrl="https://parasaur.example.test/" />
  );
  await expect(component.getByText('#43', { exact: true })).toHaveCount(2);

  // The late issue-42 response is ignored: 43's runs stay, 42's never land.
  resolveIssue42(feed);
  await page.waitForTimeout(500);
  await expect(component.getByText('#43', { exact: true })).toHaveCount(2);
  await expect(component.getByText('#42', { exact: true })).toHaveCount(0);
});
