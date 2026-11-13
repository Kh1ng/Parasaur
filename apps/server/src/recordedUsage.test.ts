import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import type { ChatAssistantMessage, DispatchUsageReport, DispatchUsageRow, RecordedUsageReport } from '@git-agent-harness/contracts';
import { observeUsage } from '@git-agent-harness/shared';
import { recordedUsageReport } from './recordedUsage.js';
import { resetCachedCoordinatorIdentity } from './coordinatorIdentity.js';
import { appendEvents, chatLogPath } from './managerChat/sessionLog.js';

const NOW = Date.parse('2026-11-06T12:00:00Z');
const DAY = 86_400_000;
const message: ChatAssistantMessage = { type: 'assistant/message', seq: 1, turn: 1, text: 'private prompt never exported',
  backend: 'codex', backendInstance: 'work', model: 'route-model', nodeId: 'worker',
  usage: { input_tokens: 80, output_tokens: 20, total_tokens: 100, cache_read_tokens: 50, estimated_cost_usd: 0.1, duration_seconds: 1 }, timestamp: NOW };
const row: DispatchUsageRow = { profile: 'project', project_provider: 'github', repo: 'owner/repo', work_id: '#42',
  backend: 'codex', backend_instance: 'work', account_label: 'Work account', auth_source_label: 'work-auth', quota_pool: 'work-pool', requested_model: 'route-model', effective_model: 'route-model',
  actual_model: 'reported-model', model_provider: 'provider', usage_classification: 'metered_api', day: '2026-11-06',
  dispatch_attempts: 1, legacy_dispatches: 0, input_tokens: 160, output_tokens: 40, total_tokens: 200,
  cache_read_tokens: 70, cache_write_tokens: null, reasoning_tokens: null, actual_cost_usd: 0.3, estimated_cost_usd: 0.2,
  measurements: observeUsage({ input_tokens: 160, output_tokens: 40, total_tokens: 200, cache_read_tokens: 70, actual_cost_usd: 0.3, estimated_cost_usd: 0.2 }) };
function native(profile: string | null = 'project', rows = [row]): DispatchUsageReport {
  return { schema_version: 1, profile, since: new Date(NOW - 7 * DAY).toISOString(), generated_at: new Date(NOW).toISOString(),
    entries: rows.length, excluded_preflight_entries: 2, rows };
}
function fixture(t: { after(fn: () => void): void }): string {
  const stateDir = mkdtempSync(join(tmpdir(), 'gah-recorded-usage-'));
  t.after(() => rmSync(stateDir, { recursive: true, force: true }));
  return stateDir;
}

test('one chat turn and one dispatch attempt on the same day sum once, with independent categories', async t => {
  const stateDir = fixture(t);
  appendEvents('project', [message, message], { stateDir, sessionId: 'session' });
  writeFileSync(join(stateDir, 'project-project', 'sessions.json'), JSON.stringify({ sessions: [{ id: 'session', branch: 'feature', issueNumber: 42 }] }));
  const report = await recordedUsageReport({ profile: 'project', days: 7 }, { stateDir, now: () => NOW, readDispatch: async () => native() });
  assert.deepEqual(report.sources, { chat: { status: 'available' }, dispatch: { status: 'available', excluded_preflight_entries: 2 } });
  assert.equal(report.rows.reduce((sum, row) => sum + row.chat_turns, 0), 1);
  assert.equal(report.rows.reduce((sum, row) => sum + row.dispatch_attempts, 0), 1);
  assert.equal(report.totals.total_tokens.reported_total, 300);
  assert.equal(report.totals.cache_read_tokens.reported_total, 120);
  assert.ok(Math.abs(report.totals.estimated_cost_usd.reported_total! - 0.3) < 1e-9);
  assert.equal(report.totals.actual_cost_usd.reported_total, 0.3);
  assert.equal(report.totals.actual_cost_usd.unmeasured_observations, 1);
  assert.equal(report.rows.find(row => row.source === 'chat')?.repo, null, 'current config must not invent historical repository identity');
  assert.equal(report.rows.find(row => row.source === 'chat')?.node_id, 'worker');
  assert.equal(report.rows.find(row => row.source === 'chat')?.work_id, '#42');
  assert.equal(report.rows.find(row => row.source === 'dispatch')?.actual_model, 'reported-model');
  assert.equal(JSON.stringify(report).includes('private prompt'), false);
});

test('missing readings retain measured subtotals and exact account/node boundaries', async t => {
  const stateDir = fixture(t);
  appendEvents('project', [message, { ...message, seq: 2, turn: 2, usage: null },
    { ...message, seq: 3, turn: 3, backendInstance: 'personal', nodeId: 'other' },
    { ...message, seq: 4, turn: 4, timestamp: NOW + 1 }], { stateDir, sessionId: 'session' });
  const report = await recordedUsageReport({ profile: 'project', days: 7 }, { stateDir, now: () => NOW, readDispatch: async () => native() });
  assert.equal(report.rows.length, 3);
  assert.equal(report.totals.total_tokens.reported_total, 400);
  assert.equal(report.totals.total_tokens.measured_observations, 3);
  assert.equal(report.totals.total_tokens.unmeasured_observations, 1);
  const work = report.rows.find(row => row.source === 'chat' && row.backend_instance === 'work')!;
  assert.equal(work.chat_turns, 2);
  assert.equal(work.measurements.total_tokens.reported_total, 100);
  assert.equal(work.measurements.total_tokens.unmeasured_observations, 1);
});

test('malformed and conflicting chat logs are unavailable, while unused sessions have no recorded turns', async t => {
  const stateDir = fixture(t);
  const deps = { stateDir, now: () => NOW, readDispatch: async () => native() };
  const path = chatLogPath('project', { stateDir, sessionId: 'session' });
  mkdirSync(join(stateDir, 'project-project', 'session-session'), { recursive: true });
  const unused = await recordedUsageReport({ profile: 'project', days: 7 }, deps);
  assert.equal(unused.sources.chat.status, 'available');
  assert.equal(unused.rows.some(row => row.source === 'chat'), false);
  for (const contents of ['{broken\n', JSON.stringify(message) + '\n' + JSON.stringify({ ...message, text: 'conflict' }) + '\n']) {
    writeFileSync(path, contents);
    const report = await recordedUsageReport({ profile: 'project', days: 7 }, deps);
    assert.equal(report.sources.chat.status, 'unavailable');
    assert.equal(report.sources.dispatch.status, 'available');
    assert.equal(report.totals.total_tokens.reported_total, 200);
    assert.equal(report.rows.some(row => row.source === 'chat'), false);
  }
});

test('an old, wrong-scope or duplicate native response cannot masquerade as a complete empty source', async t => {
  const stateDir = fixture(t);
  appendEvents('project', [message], { stateDir, sessionId: 'session' });
  for (const response of [native('other'), native('project', [row, row]), native('project', [{ ...row, measurements: undefined } as unknown as DispatchUsageRow])]) {
    const report = await recordedUsageReport({ profile: 'project', days: 7 }, { stateDir, now: () => NOW, readDispatch: async () => response });
    assert.equal(report.sources.dispatch.status, 'unavailable');
    assert.equal(report.sources.chat.status, 'available');
    assert.equal(report.totals.total_tokens.reported_total, 100);
  }
});

test('all-project scope includes archived logs and keeps empty sources distinct from missing measurements', async t => {
  const stateDir = fixture(t);
  for (const project of ['project', 'other']) appendEvents(project, [message], { stateDir, sessionId: 'archive' });
  const all = await recordedUsageReport({ days: 7 }, { stateDir, now: () => NOW, readDispatch: async () => native(null, []) });
  assert.equal(all.rows.length, 2);
  assert.equal(all.totals.total_tokens.reported_total, 200);
  const empty = await recordedUsageReport({ profile: 'missing', days: 7 }, { stateDir, now: () => NOW, readDispatch: async () => native('missing', []) });
  assert.equal(empty.sources.chat.status, 'available');
  assert.equal(empty.sources.dispatch.status, 'available');
  assert.equal(empty.totals.total_tokens.reported_total, null);
});

test('HTTP combined report preserves full source coverage and rejects invalid windows', async t => {
  const stateDir = fixture(t);
  const previousIdentity = process.env.PARASAUR_COORDINATOR_IDENTITY_PATH;
  process.env.PARASAUR_COORDINATOR_IDENTITY_PATH = join(stateDir, 'identity.json');
  resetCachedCoordinatorIdentity();
  t.after(() => {
    if (previousIdentity === undefined) delete process.env.PARASAUR_COORDINATOR_IDENTITY_PATH;
    else process.env.PARASAUR_COORDINATOR_IDENTITY_PATH = previousIdentity;
    resetCachedCoordinatorIdentity();
  });
  const { createServer } = await import('./server.js');
  appendEvents('project', [message], { stateDir, sessionId: 'session' });
  const server = http.createServer(createServer({ usageReport: { stateDir, now: () => NOW, readDispatch: async () => native() } }));
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise<void>(resolve => server.close(() => resolve())));
  const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}/api/usage/report`;
  const response = await fetch(base + '?profile=project&days=7');
  assert.equal(response.status, 200);
  const body = await response.json() as RecordedUsageReport;
  assert.equal(body.totals.total_tokens.reported_total, 300);
  for (const query of ['days=0', 'days=91', 'days=no', 'days=1.5', 'profile=']) assert.equal((await fetch(base + '?' + query)).status, 400);
});
