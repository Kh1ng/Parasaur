import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import type { ControllerEvent, LedgerEntry, ProfileSummary, Session } from '@git-agent-harness/contracts';
import { withFixtureServer } from '../../fixtureGahHarness.js';
import {
  GITLAB_RUNS_FEED_DEFAULT_LIMIT,
  buildGitLabRunsFeed,
  ledgerEntryMatchesGitLabContext,
  matchingGitLabProfiles,
  mrIidFromUrl,
  parseGitLabRunsQuery,
  sessionMatchesGitLabContext,
  type GitLabRunsContext
} from './runsFeed.js';

const context = (overrides: Partial<GitLabRunsContext> = {}): GitLabRunsContext => ({
  project: 'parasaur-test/group/project',
  kind: 'issue',
  iid: 42,
  origin: null,
  branch: null,
  limit: GITLAB_RUNS_FEED_DEFAULT_LIMIT,
  ...overrides
});

function ledgerEntry(overrides: Partial<LedgerEntry> = {}): LedgerEntry {
  return {
    timestamp: '2026-10-20T10:00:00Z',
    session_id: null,
    profile: 'gl-profile',
    display_name: 'Group Project',
    repo_id: 'project',
    repo: 'parasaur-test/group/project',
    local_path: '/repo',
    provider: 'gitlab',
    backend: 'codex',
    requested_backend: 'codex',
    effective_backend: 'codex',
    requested_model: null,
    effective_model: 'gpt-6.1',
    routing_reason: null,
    fallback_used: false,
    confidence_impact: null,
    human_required: false,
    mode: 'improve',
    target_summary: null,
    branch: null,
    session_dir: null,
    duration_seconds: 120,
    backend_exit_code: 0,
    validation_result: 'passed',
    commit_attempted: true,
    commit_created: true,
    push_attempted: true,
    push_succeeded: true,
    mr_attempted: true,
    mr_created: true,
    mr_url: 'https://gitlab.example.test/parasaur-test/group/project/-/merge_requests/7',
    files_changed: 2,
    insertions: 10,
    deletions: 1,
    error_summary: null,
    usage: { usage_source: null, input_tokens: null, output_tokens: null, cache_read_tokens: null, cache_write_tokens: null, total_tokens: null, requests_count: null, estimated_cost_usd: null, actual_cost_usd: null },
    ...overrides
  };
}

function liveSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'session-1',
    providerKind: 'gitlab',
    instanceId: 'gitlab-0',
    status: 'running',
    repo: 'parasaur-test/group/project',
    mode: 'improve',
    target: '#42',
    backend: 'codex',
    startedAt: '2026-10-24T09:00:00Z',
    ...overrides
  };
}

const GITLAB_PROFILE: ProfileSummary = {
  name: 'gl-profile',
  display_name: 'Group Project',
  provider: 'gitlab',
  repo: 'parasaur-test/group/project',
  local_path: '/repo',
  repo_id: 'project',
  worktree_base: '',
  web_url: 'https://gitlab.example.test/parasaur-test/group/project',
  max_parallel_workers: null,
  max_open_managed_mrs: 1,
  delivery_mode: 'pr'
} as unknown as ProfileSummary;

test('parseGitLabRunsQuery accepts a complete context and applies defaults', () => {
  const parsed = parseGitLabRunsQuery({ project: 'parasaur-test/group/project', kind: 'issue', iid: '42' });
  assert.deepEqual(parsed, { project: 'parasaur-test/group/project', kind: 'issue', iid: 42, origin: null, branch: null, limit: 20 });
  const full = parseGitLabRunsQuery({
    project: 'parasaur-test/group/project', kind: 'merge_request', iid: '7',
    gitlab: 'https://gitlab.example.test', branch: 'gah/issue/project-42', limit: '5'
  });
  assert.deepEqual(full, {
    project: 'parasaur-test/group/project', kind: 'merge_request', iid: 7,
    origin: 'https://gitlab.example.test', branch: 'gah/issue/project-42', limit: 5
  });
});

test('parseGitLabRunsQuery rejects an empty or malformed context', () => {
  for (const [name, params] of [
    ['no parameters at all', {}],
    ['missing project', { kind: 'issue', iid: '42' }],
    ['blank project', { project: '  ', kind: 'issue', iid: '42' }],
    ['missing kind', { project: 'a/b', iid: '42' }],
    ['unknown kind', { project: 'a/b', kind: 'pipeline', iid: '42' }],
    ['missing iid', { project: 'a/b', kind: 'issue' }],
    ['zero iid', { project: 'a/b', kind: 'issue', iid: '0' }],
    ['non-numeric iid', { project: 'a/b', kind: 'issue', iid: 'forty-two' }],
    ['zero limit', { project: 'a/b', kind: 'issue', iid: '42', limit: '0' }],
    ['limit above the cap', { project: 'a/b', kind: 'issue', iid: '42', limit: '101' }],
    ['malformed origin', { project: 'a/b', kind: 'issue', iid: '42', gitlab: 'not-a-url' }],
    ['control characters in project', { project: 'a/b\u0000', kind: 'issue', iid: '42' }]
  ] as const) {
    const parsed = parseGitLabRunsQuery(params as Record<string, unknown>);
    assert.equal('error' in parsed, true, `${name} must be rejected`);
  }
});

test('mrIidFromUrl reads the iid from a GitLab MR URL only', () => {
  assert.equal(mrIidFromUrl('https://gitlab.example.test/group/project/-/merge_requests/42'), 42);
  assert.equal(mrIidFromUrl('https://gitlab.example.test/group/project/-/merge_requests/42/diffs'), 42);
  assert.equal(mrIidFromUrl('https://github.com/owner/repo/pull/42'), null);
  assert.equal(mrIidFromUrl('https://gitlab.example.test/group/project/-/issues/42'), null);
  assert.equal(mrIidFromUrl(null), null);
  assert.equal(mrIidFromUrl('not a url'), null);
});

test('sessions match through the structured GitLab context recorded by the dispatcher', () => {
  const session = liveSession({
    target: 'GitLab instruction for https://gitlab.example.test/parasaur-test/group/project/-/issues/42:',
    gitlab: { project: 'parasaur-test/group/project', kind: 'issue', iid: 42, webUrl: 'https://gitlab.example.test/parasaur-test/group/project' }
  });
  assert.equal(sessionMatchesGitLabContext(session, context()), true);
  assert.equal(sessionMatchesGitLabContext(session, context({ iid: 43 })), false);
  assert.equal(sessionMatchesGitLabContext(session, context({ kind: 'merge_request' })), false);
  assert.equal(sessionMatchesGitLabContext(session, context({ project: 'other/project' })), false);
  // An origin demand must be proven by the recorded context.
  assert.equal(sessionMatchesGitLabContext(session, context({ origin: 'https://gitlab.example.test' })), true);
  assert.equal(sessionMatchesGitLabContext(session, context({ origin: 'https://other.example.test' })), false);
});

test('sessions started before the context field exist are derived from repo, target and mr', () => {
  assert.equal(sessionMatchesGitLabContext(liveSession({ target: '#42' }), context()), true);
  assert.equal(sessionMatchesGitLabContext(liveSession({ target: '42' }), context()), true);
  assert.equal(sessionMatchesGitLabContext(liveSession({
    target: 'GitLab instruction for https://gitlab.example.test/parasaur-test/group/project/-/issues/42:\n\nfix this'
  }), context()), true);
  // iid 421 must not match issue 42.
  assert.equal(sessionMatchesGitLabContext(liveSession({ target: '#421' }), context()), false);
  assert.equal(sessionMatchesGitLabContext(liveSession({ target: '#42' }), context({ kind: 'merge_request' })), false);
  assert.equal(sessionMatchesGitLabContext(liveSession({ providerKind: 'github', target: '#42' }), context()), false);
  assert.equal(sessionMatchesGitLabContext(liveSession({ repo: 'other/project', target: '#42' }), context()), false);
  // A derived session cannot prove a demanded origin.
  assert.equal(sessionMatchesGitLabContext(liveSession({ target: '#42' }), context({ origin: 'https://gitlab.example.test' })), false);
  // Branch narrowing applies to derived matches.
  assert.equal(sessionMatchesGitLabContext(liveSession({ target: '#42', branch: 'gah/issue/project-42' }), context({ branch: 'gah/issue/project-42' })), true);
  assert.equal(sessionMatchesGitLabContext(liveSession({ target: '#42', branch: 'main' }), context({ branch: 'gah/issue/project-42' })), false);
});

test('merge request sessions match by mr number or instruction URL', () => {
  const mrContext = context({ kind: 'merge_request', iid: 7 });
  assert.equal(sessionMatchesGitLabContext(liveSession({ target: undefined, mr: '7' }), mrContext), true);
  assert.equal(sessionMatchesGitLabContext(liveSession({
    target: 'GitLab instruction for https://gitlab.example.test/parasaur-test/group/project/-/merge_requests/7:',
    mr: '7'
  }), mrContext), true);
  assert.equal(sessionMatchesGitLabContext(liveSession({ target: undefined, mr: '8' }), mrContext), false);
  assert.equal(sessionMatchesGitLabContext(liveSession({ target: undefined, mr: '71' }), mrContext), false);
});

test('ledger entries match by the identities the ledger already records', () => {
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ source_issue_number: '42' }), context()), true);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ work_id: '#42' }), context()), true);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({
    target_summary: 'GitLab instruction for https://gitlab.example.test/parasaur-test/group/project/-/issues/42:'
  }), context()), true);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ source_issue_number: '421' }), context()), false);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ source_issue_number: '42' }), context({ kind: 'merge_request' })), false);
  // A GitHub entry for the same issue number must stay out.
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ provider: 'github', repo: 'owner/repo', source_issue_number: '42' }), context()), false);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ repo: 'other/project', source_issue_number: '42' }), context()), false);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ mode: 'review_hold', source_issue_number: '42' }), context()), false);
  // Branch narrowing.
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ source_issue_number: '42', branch: 'gah/issue/project-42' }), context({ branch: 'gah/issue/project-42' })), true);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ source_issue_number: '42', branch: 'main' }), context({ branch: 'gah/issue/project-42' })), false);
});

test('merge request ledger entries match by mr work id or MR URL', () => {
  const mrContext = context({ kind: 'merge_request', iid: 7 });
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ mode: 'review', work_id: 'mr:7' }), mrContext), true);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ mode: 'review', work_id: 'mr:71', mr_url: null }), mrContext), false);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({
    mode: 'review', work_id: null, mr_url: 'https://gitlab.example.test/parasaur-test/group/project/-/merge_requests/7'
  }), mrContext), true);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({
    mode: 'review', work_id: null, mr_url: 'https://gitlab.example.test/parasaur-test/group/project/-/merge_requests/71'
  }), mrContext), false);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({
    mode: 'review', work_id: null, mr_url: null,
    target_summary: 'GitLab instruction for https://gitlab.example.test/parasaur-test/group/project/-/merge_requests/7:'
  }), mrContext), true);
  assert.equal(ledgerEntryMatchesGitLabContext(ledgerEntry({ mode: 'review', work_id: 'mr:7' }), context()), false);
});

test('profile matching narrows by provider, project path and origin', () => {
  const githubTwin = { ...GITLAB_PROFILE, name: 'gh-profile', provider: 'github', web_url: 'https://github.com/parasaur-test/group/project' };
  const otherOrigin = { ...GITLAB_PROFILE, name: 'gl-other-host', web_url: 'https://gitlab.other.test/parasaur-test/group/project' };
  const profiles = [GITLAB_PROFILE, githubTwin, otherOrigin];
  assert.deepEqual(matchingGitLabProfiles(profiles, context()).map(profile => profile.name), ['gl-profile', 'gl-other-host']);
  assert.deepEqual(
    matchingGitLabProfiles(profiles, context({ origin: 'https://gitlab.other.test' })).map(profile => profile.name),
    ['gl-other-host']
  );
});

test('buildGitLabRunsFeed returns only the context runs, newest first, bounded', () => {
  const older = ledgerEntry({ source_issue_number: '42', timestamp: '2026-10-20T10:00:00Z' });
  const newer = ledgerEntry({
    source_issue_number: '42', timestamp: '2026-10-22T10:00:00Z',
    validation_result: 'failed', error_summary: 'tests failed',
    attempts: [{ attempt_number: 1, backend: 'codex', effective_model: 'gpt-6.1', exit_code: 1, validation_result: 'failed', failure_class: 'test_failure', failure_stage: 'validate', duration_seconds: 60, diff_path: null, usage: older.usage }]
  });
  const githubEntry = ledgerEntry({ provider: 'github', repo: 'owner/repo', source_issue_number: '42', timestamp: '2026-10-23T10:00:00Z' });
  const mrEntry = ledgerEntry({ mode: 'review', work_id: 'mr:7', timestamp: '2026-10-21T10:00:00Z' });
  const boundary = ledgerEntry({ source_issue_number: '421', timestamp: '2026-10-24T10:00:00Z' });
  const live = liveSession({
    id: 'live-42', startedAt: '2026-10-25T08:00:00Z',
    gitlab: { project: 'parasaur-test/group/project', kind: 'issue', iid: 42, webUrl: 'https://gitlab.example.test/parasaur-test/group/project' }
  });
  const terminal = liveSession({ id: 'done-42', status: 'stopped', target: '#42' });
  const wrongProject = liveSession({ id: 'other', repo: 'other/project', target: '#42' });

  const events: Record<string, ControllerEvent[]> = {
    'gl-profile': [
      { timestamp: '2026-10-22T11:00:00Z', event_type: 'dispatch_finished', profile: 'gl-profile', work_id: '#42', details: '#42: validation passed'.padEnd(220, 'x') },
      { timestamp: '2026-10-22T11:30:00Z', event_type: 'dispatch_finished', profile: 'gl-profile', work_id: '#999', details: 'unrelated work' },
      { timestamp: '2026-10-22T12:00:00Z', event_type: 'dispatch_started', profile: 'gl-profile', work_id: '#42', details: 'start' }
    ]
  };

  const feed = buildGitLabRunsFeed(
    { profiles: [GITLAB_PROFILE], sessions: [live, terminal, wrongProject], ledgerEntries: [githubEntry, older, newer, mrEntry, boundary], events },
    context()
  );

  assert.deepEqual(feed.context, {
    project: 'parasaur-test/group/project', kind: 'issue', iid: 42,
    webUrl: 'https://gitlab.example.test/parasaur-test/group/project', profiles: ['gl-profile']
  });
  assert.deepEqual(feed.runs.map(run => run.id), ['live-42', 'ledger:gl-profile:2026-10-22T10:00:00Z:improve', 'ledger:gl-profile:2026-10-20T10:00:00Z:improve']);
  assert.equal(feed.runs[0].source, 'session');
  assert.equal(feed.runs[0].profile, 'gl-profile');
  assert.equal(feed.runs[1].status, 'failed');
  assert.equal(feed.runs[1].testStatus, 'failed');
  assert.equal(feed.runs[2].status, 'passed');
  assert.equal(feed.runs[2].mrUrl, 'https://gitlab.example.test/parasaur-test/group/project/-/merge_requests/7');
  assert.deepEqual(feed.runs[1].attempts, [{
    attempt: 1, backend: 'codex', model: 'gpt-6.1', exitCode: 1,
    validationResult: 'failed', failureClass: 'test_failure', durationSeconds: 60
  }]);

  // Events narrow to the matched work ids, newest first, with truncated details.
  assert.deepEqual(feed.events.map(event => event.eventType), ['dispatch_started', 'dispatch_finished']);
  assert.equal(feed.events[1].workId, '#42');
  assert.equal(feed.events[1].message.length, 201);
  assert.ok(feed.events[1].message.endsWith('…'));

  // The bound is applied after sorting: limit 1 keeps the newest run only.
  const bounded = buildGitLabRunsFeed(
    { profiles: [GITLAB_PROFILE], sessions: [live], ledgerEntries: [older, newer], events },
    context({ limit: 1 })
  );
  assert.deepEqual(bounded.runs.map(run => run.id), ['live-42']);
});

test('an origin demand excludes ledger entries whose profile cannot prove that host', () => {
  const entry = ledgerEntry({ source_issue_number: '42' });
  const otherHost = { ...GITLAB_PROFILE, name: 'gl-other-host', web_url: 'https://gitlab.other.test/parasaur-test/group/project' };
  const feed = buildGitLabRunsFeed(
    { profiles: [GITLAB_PROFILE, otherHost], sessions: [], ledgerEntries: [entry], events: {} },
    context({ origin: 'https://gitlab.other.test' })
  );
  // The entry's profile is gl-profile on a different host: excluded.
  assert.deepEqual(feed.runs, []);
  assert.deepEqual(feed.context.profiles, ['gl-other-host']);
});

test('GET /api/integrations/gitlab/runs filters mixed ledger fixtures through the real route', async () => {
  const root = mkdtempSync(join(tmpdir(), 'gah-gitlab-runs-'));
  const profilesPath = join(root, 'profiles.json');
  const reportPath = join(root, 'report.json');
  const ledgerPath = join(root, 'ledger.jsonl');
  writeFileSync(profilesPath, JSON.stringify([
    {
      name: 'gl-profile', display_name: 'Group Project', provider: 'gitlab',
      repo: 'parasaur-test/group/project', local_path: '/repo', repo_id: 'project', worktree_base: '',
      web_url: 'https://gitlab.example.test/parasaur-test/group/project',
      max_parallel_workers: null, max_open_managed_mrs: 1, delivery_mode: 'pr'
    },
    {
      name: 'gh-profile', display_name: 'Twin', provider: 'github',
      repo: 'parasaur-test/group/project', local_path: '/repo', repo_id: 'project', worktree_base: '',
      web_url: 'https://github.com/parasaur-test/group/project',
      max_parallel_workers: null, max_open_managed_mrs: 1, delivery_mode: 'pr'
    }
  ]));
  writeFileSync(reportPath, JSON.stringify({ ledger_path: ledgerPath, total_entries: 4, since: '1d', profile: null, group_by: 'Backend', comparisons: [] }));
  writeFileSync(ledgerPath, [
    JSON.stringify({ ...ledgerEntry(), source_issue_number: '42', timestamp: '2026-10-20T10:00:00Z', validation_result: 'passed', profile: 'gl-profile' }),
    JSON.stringify({ ...ledgerEntry(), provider: 'github', repo: 'owner/repo', source_issue_number: '42', timestamp: '2026-10-23T10:00:00Z', profile: 'gh-profile' }),
    JSON.stringify({ ...ledgerEntry(), mode: 'review', work_id: 'mr:7', timestamp: '2026-10-21T10:00:00Z', profile: 'gl-profile' }),
    JSON.stringify({ ...ledgerEntry(), source_issue_number: '42', timestamp: '2026-10-22T10:00:00Z', validation_result: 'failed', error_summary: 'tests failed', profile: 'gl-profile' })
  ].join('\n') + '\n');

  const saved = {
    GAH_FIXTURE_PROFILE_LIST: process.env.GAH_FIXTURE_PROFILE_LIST,
    GAH_FIXTURE_REPORT: process.env.GAH_FIXTURE_REPORT
  };
  process.env.GAH_FIXTURE_PROFILE_LIST = profilesPath;
  process.env.GAH_FIXTURE_REPORT = reportPath;
  try {
    await withFixtureServer(async (baseUrl) => {
      const feed = (query: string) => fetch(`${baseUrl}/api/integrations/gitlab/runs${query}`);

      // An empty context is a 400, never a widened feed.
      for (const [name, query] of [
        ['no context', ''],
        ['missing iid', '?project=parasaur-test/group/project&kind=issue'],
        ['unknown kind', '?project=parasaur-test/group/project&kind=pipeline&iid=42'],
        ['limit above the cap', '?project=parasaur-test/group/project&kind=issue&iid=42&limit=101']
      ] as const) {
        const response = await feed(query);
        assert.equal(response.status, 400, `${name} must be a 400`);
        const body = await response.json() as { error: string; message: string };
        assert.equal(body.error, 'invalid_gitlab_context');
        assert.ok(body.message.length > 0);
      }

      const response = await feed('?project=parasaur-test%2Fgroup%2Fproject&kind=issue&iid=42');
      assert.equal(response.status, 200);
      const body = await response.json() as {
        context: { webUrl: string | null; profiles: string[] };
        runs: { id: string; status: string; testStatus: string | null; profile: string; mrUrl: string | null }[];
        events: unknown[];
      };
      assert.deepEqual(body.context.profiles, ['gl-profile']);
      assert.equal(body.context.webUrl, 'https://gitlab.example.test/parasaur-test/group/project');
      // Only this issue's GitLab runs, newest first; the GitHub twin and MR 7 stay out.
      assert.deepEqual(body.runs.map(run => run.id), [
        'ledger:gl-profile:2026-10-22T10:00:00Z:improve',
        'ledger:gl-profile:2026-10-20T10:00:00Z:improve'
      ]);
      assert.deepEqual(body.runs.map(run => run.status), ['failed', 'passed']);
      assert.equal(body.runs[0].testStatus, 'failed');
      assert.ok(body.runs[1].mrUrl?.endsWith('/-/merge_requests/7'));
      assert.equal(body.events.length, 0); // The fixture records no controller events.

      const bounded = await feed('?project=parasaur-test%2Fgroup%2Fproject&kind=issue&iid=42&limit=1');
      const boundedBody = await bounded.json() as { runs: { id: string }[] };
      assert.deepEqual(boundedBody.runs.map(run => run.id), ['ledger:gl-profile:2026-10-22T10:00:00Z:improve']);

      const mergeRequest = await feed('?project=parasaur-test%2Fgroup%2Fproject&kind=merge_request&iid=7');
      const mrBody = await mergeRequest.json() as { runs: { id: string; workId: string | null }[] };
      // The MR context matches its review run and the issue runs that
      // produced MR 7 (ledger mr_url); the GitHub twin stays out.
      assert.deepEqual(mrBody.runs.map(run => run.id), [
        'ledger:gl-profile:2026-10-22T10:00:00Z:improve',
        'ledger:gl-profile:2026-10-21T10:00:00Z:review',
        'ledger:gl-profile:2026-10-20T10:00:00Z:improve'
      ]);
      assert.equal(mrBody.runs.find(run => run.id.endsWith(':review'))?.workId, 'mr:7');

      // A context with no matching profile is an empty feed, not an error.
      const unknown = await feed('?project=nope/unknown&kind=issue&iid=1');
      assert.equal(unknown.status, 200);
      const unknownBody = await unknown.json() as { runs: unknown[]; context: { profiles: string[] } };
      assert.deepEqual(unknownBody.runs, []);
      assert.deepEqual(unknownBody.context.profiles, []);

      // The dispatch route applies the same validation to a start request's
      // context: a malformed context is rejected before anything starts.
      const dispatch = (gitlab: unknown) => fetch(`${baseUrl}/api/dispatch`, {
        method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ profile: 'gl-profile', providerKind: 'gitlab', instanceId: 'gitlab-0', repo: 'parasaur-test/group/project', mode: 'improve', target: '#42', gitlab })
      });
      assert.equal((await dispatch({ project: 'parasaur-test/group/project', kind: 'issue', iid: 42, webUrl: 'not-a-url' })).status, 400);
      assert.equal((await dispatch({ project: 'parasaur-test/group/project', kind: 'epic', iid: 42, webUrl: 'https://gitlab.example.test/a/b' })).status, 400);
      assert.equal((await dispatch('not-an-object')).status, 400);
    });
  } finally {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    rmSync(root, { recursive: true, force: true });
  }
});
