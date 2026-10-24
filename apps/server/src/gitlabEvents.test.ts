import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Request, Response, NextFunction } from 'express';
import { mkdtempSync, rmSync, readdirSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { gitlabEventsRouter, hookTask, authorize, type GitlabHookProject } from './gitlabEvents.js';

const project: GitlabHookProject = { projectId: 42, profile: 'test', repo: 'group/repo', apiBase: 'https://gitlab.example/api/v4', bot: 'parasaur', providerKind: 'codex', instanceId: 'codex-0' };
const fixture = (name: string) => JSON.parse(readFileSync(new URL(`./fixtures/gitlab-19/${name}.json`, import.meta.url), 'utf8'));
const issue = fixture('issue');

test('GitLab issue assignment, review request, note and pipeline selection', () => {
  assert.deepEqual(hookTask(fixture('merge_request'), project), { kind: 'merge_requests', iid: 8 });
  assert.deepEqual(hookTask(fixture('note'), project), { kind: 'issues', iid: 9, instruction: '@parasaur fix this' });
  assert.equal(hookTask(fixture('pipeline'), project), null);
  assert.deepEqual(hookTask(issue, project), { kind: 'issues', iid: 7 });
  assert.equal(hookTask({ ...issue, changes: {} }, project), null);
  assert.deepEqual(hookTask({ object_kind: 'merge_request', object_attributes: { iid: 8, state: 'opened' }, changes: { reviewers: { previous: [], current: [{ username: 'parasaur' }] } } }, project), { kind: 'merge_requests', iid: 8 });
  for (const [noteable_type, key, kind] of [['Issue', 'issue', 'issues'], ['MergeRequest', 'merge_request', 'merge_requests']]) {
    assert.deepEqual(hookTask({ object_kind: 'note', object_attributes: { noteable_type, note: '@parasaur fix this' }, [key!]: { iid: 9 } }, project), { kind, iid: 9, instruction: '@parasaur fix this' });
  }
  assert.equal(hookTask({ object_kind: 'pipeline', object_attributes: { status: 'failed', ref: 'gah/owned' } }, project), null);
  assert.equal(hookTask({ object_kind: 'note', object_attributes: { note: '@parasaurish hi', noteable_type: 'Issue' }, issue: { iid: 7 } }, project), null);
});

test('hook authentication, async dispatch, durable deduplication and comment', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'gitlab-hook-'));
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  const calls: unknown[] = [];
  const comments: string[] = [];
  const deps = { secret: 'secret', projects: [project], stateDir: directory, panelUrl: 'https://panel.example', authorize: async () => { await gate; return true; }, startSession: async (options: unknown) => { calls.push(options); return { id: 'run-1' }; }, comment: async (_p: unknown, _t: unknown, text: string) => { comments.push(text); } };
  const send = (secret?: string, router = gitlabEventsRouter(deps)) => invoke(router, issue, secret);
  try {
    assert.equal((await send()).status, 401);
    assert.equal((await send('wrong')).status, 401);
    assert.equal((await send('secret')).status, 202);
    assert.equal(calls.length, 0);
    assert.equal((await send('secret')).status, 200);
    release();
    for (let i = 0; i < 100 && !comments.length; i++) await new Promise(resolve => setTimeout(resolve, 10));
    assert.equal(calls.length, 1); assert.match(comments[0]!, /https:\/\/panel.example\/\?session=run-1/);
    assert.match(readFileSync(join(directory, readdirSync(directory)[0]!), 'utf8'), /queued/);
    // A fresh router uses the same receipt, including after a server restart.
    const duplicate = await send('secret', gitlabEventsRouter(deps));
    assert.equal(duplicate.status, 200); assert.deepEqual(await duplicate.json(), { duplicate: true });
    assert.equal(calls.length, 1);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

function invoke(router: ReturnType<typeof gitlabEventsRouter>, body: unknown, secret?: string): Promise<{ status: number; json: () => unknown }> {
  return new Promise((resolve, reject) => {
    let status = 200;
    const req = { method: 'POST', url: '/api/integrations/gitlab/events', body, get: (key: string) => key === 'X-Parasaur-Hook-Secret' ? secret : undefined };
    const res = { status: (value: number) => { status = value; return res; }, json: (value: unknown) => resolve({ status, json: () => value }) };
    router(req as Request, res as unknown as Response, reject as NextFunction);
  });
}

test('actor trust preserves unknown type and profile human/bot separation', async () => {
  const config = async () => ({ trusted_issue_human_authors: ['OWNER'], trusted_issue_bot_authors: ['service'] }) as Awaited<ReturnType<typeof import('./gahCli.js').runConfigShowProfile>>;
  for (const [actor, expected] of [
    [{ id: 1, username: 'owner', bot: false }, true],
    [{ id: 1, username: 'owner' }, false],
    [{ id: 1, username: 'owner', bot: true }, false],
    [{ id: 1, username: 'other', bot: false }, false],
  ] as const) assert.equal(await authorize(project, issue, { config, user: async () => actor }), expected);
  assert.equal(await authorize(project, { ...issue, object_attributes: { ...issue.object_attributes, author_id: 3 } }, { config, user: async id => ({ id, username: id === 1 ? 'owner' : 'untrusted', bot: false }) }), false);
});

test('untrusted events never dispatch and uncertain failures retain receipts', async () => {
  for (const allowed of [false, true]) {
    const directory = mkdtempSync(join(tmpdir(), 'gitlab-hook-'));
    let calls = 0;
    try {
      const router = gitlabEventsRouter({ secret: 'secret', projects: [project], stateDir: directory, authorize: async () => allowed,
        startSession: async () => { calls++; throw new Error('uncertain'); } });
      assert.equal((await invoke(router, issue, 'secret')).status, 202);
      let receipt = '';
      for (let i = 0; i < 100; i++) {
        receipt = readFileSync(join(directory, readdirSync(directory)[0]!), 'utf8');
        if (!receipt.includes('accepted')) break;
        await new Promise(resolve => setTimeout(resolve, 10));
      }
      assert.match(receipt, allowed ? /needs_review/ : /ignored_untrusted_author/);
      assert.equal((await invoke(router, issue, 'secret')).status, 200);
      assert.equal(calls, allowed ? 1 : 0);
    } finally { rmSync(directory, { recursive: true, force: true }); }
  }
});

test('MR review and note instructions reach the worker; comment failures preserve the queued run', async () => {
  for (const name of ['merge_request', 'note']) {
    const directory = mkdtempSync(join(tmpdir(), 'gitlab-hook-'));
    const calls: Record<string, unknown>[] = [];
    try {
      const router = gitlabEventsRouter({ secret: 'secret', projects: [project], stateDir: directory, authorize: async () => true,
        startSession: async options => { calls.push(options); return { id: 'queued-run' }; },
        comment: async () => { throw new Error('notification failed'); } });
      assert.equal((await invoke(router, fixture(name), 'secret')).status, 202);
      let receipt = '';
      for (let i = 0; i < 100; i++) {
        receipt = readFileSync(join(directory, readdirSync(directory)[0]!), 'utf8');
        if (receipt.includes('queued_comment_failed')) break;
        await new Promise(resolve => setTimeout(resolve, 10));
      }
      assert.match(receipt, /queued_comment_failed/); assert.match(receipt, /queued-run/);
      assert.equal(calls.length, 1);
      assert.equal(calls[0]!.profile, project.profile);
      if (name === 'merge_request') { assert.equal(calls[0]!.mode, 'review'); assert.equal(calls[0]!.mr, '8'); }
      else { assert.equal(calls[0]!.mode, 'fix'); assert.match(String(calls[0]!.target), /@parasaur fix this/); }
    } finally { rmSync(directory, { recursive: true, force: true }); }
  }
});

test('irrelevant, malformed and unconfigured-project events are ignored without dispatch', async () => {
  let calls = 0;
  const router = gitlabEventsRouter({ secret: 'secret', projects: [project], startSession: async () => { calls++; return { id: 'unexpected' }; } });
  for (const payload of [null, [], {}, fixture('pipeline'), { ...issue, project: { id: 999 } }]) {
    const response = await invoke(router, payload, 'secret');
    assert.equal(response.status, 200); assert.deepEqual(response.json(), { ignored: true });
  }
  assert.equal(calls, 0);
});
