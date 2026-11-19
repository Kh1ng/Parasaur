import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { usageRollup, ticketLabelForBranch } from './usageRollup.js';
import { setChatSessionStoreOptions } from './chatSessions.js';
import { appendEvents } from './sessionLog.js';

const NOW = Date.parse('2026-08-30T12:00:00Z');
const DAY = 86_400_000;

test('partial, absent and overlapping categories never manufacture complete totals', () => {
  const stateDir = fixture();
  try {
    sessionLog(stateDir, 'repo', 'partial');
    const usage = { input_tokens: 10, output_tokens: 5, total_tokens: 100, cache_read_tokens: 80,
      cache_write_tokens: 0, reasoning_tokens: 3, estimated_cost_usd: 0, duration_seconds: 1 };
    appendEvents('repo', [
      { type: 'assistant/message', seq: 1, turn: 1, text: 'known', backend: 'codex', model: 'complete', usage, timestamp: NOW },
      { type: 'assistant/message', seq: 2, turn: 2, text: 'partial', backend: 'codex', model: 'partial', usage: { ...usage, input_tokens: null, total_tokens: null }, timestamp: NOW },
      { type: 'assistant/message', seq: 3, turn: 3, text: 'known', backend: 'codex', model: 'mixed', usage, timestamp: NOW },
      { type: 'assistant/message', seq: 4, turn: 4, text: 'unknown', backend: 'codex', model: 'mixed', usage: null, timestamp: NOW },
      { type: 'assistant/message', seq: 5, turn: 5, text: 'future', backend: 'codex', model: 'future', usage, timestamp: NOW + 1000 },
      { type: 'assistant/message', seq: 6, turn: 6, text: 'cost only', backend: 'codex', model: 'cost-only', usage: { input_tokens: null, output_tokens: null, total_tokens: null, estimated_cost_usd: 0.25, duration_seconds: 1 }, timestamp: NOW },
      { type: 'assistant/message', seq: 7, turn: 7, text: 'invalid', backend: 'codex', model: 'invalid', usage: { ...usage, input_tokens: -1, total_tokens: Infinity, estimated_cost_usd: -1 }, timestamp: NOW }
    ], { stateDir, sessionId: 'partial' });
    const rollup = usageRollup('repo', 7, { stateDir, now: () => NOW });
    const complete = rollup.rows.find(row => row.model === 'complete')!;
    assert.equal(complete.total_tokens, 100);
    assert.equal(complete.cache_read_tokens, 80);
    assert.equal(complete.cache_write_tokens, 0);
    assert.equal(complete.reasoning_tokens, 3);
    assert.equal(complete.estimated_cost_usd, 0);
    const partial = rollup.rows.find(row => row.model === 'partial')!;
    assert.equal(partial.total_tokens, null);
    assert.equal(partial.input_tokens, null);
    assert.equal(partial.output_tokens, 5);
    assert.equal(partial.cache_read_tokens, 80);
    const mixed = rollup.rows.find(row => row.model === 'mixed')!;
    assert.equal(mixed.turns, 2);
    assert.equal(mixed.total_tokens, null);
    assert.equal(mixed.estimated_cost_usd, null);
    assert.equal(rollup.rows.some(row => row.model === 'future'), false);
    assert.equal(rollup.rows.find(row => row.model === 'cost-only')?.estimated_cost_usd, 0.25);
    assert.equal(rollup.rows.find(row => row.model === 'invalid')?.input_tokens, null);
    assert.equal(rollup.rows.find(row => row.model === 'invalid')?.total_tokens, null);
    assert.equal(rollup.rows.find(row => row.model === 'invalid')?.estimated_cost_usd, null);
    assert.equal(rollup.tickets[0].turns, 6);
    assert.equal(rollup.tickets[0].total_tokens, null);
    assert.equal(rollup.tickets[0].backends.codex, null);
  } finally {
    setChatSessionStoreOptions({ stateDir: undefined });
    rmSync(stateDir, { recursive: true, force: true });
  }
});

function fixture(): string {
  const stateDir = join(tmpdir(), `gah-usage-rollup-${Date.now()}-${Math.random().toString(36).slice(2)}`);
  mkdirSync(stateDir, { recursive: true });
  setChatSessionStoreOptions({ stateDir });
  return stateDir;
}

function sessionLog(stateDir: string, profile: string, sessionId: string): void {
  const dir = join(stateDir, `project-${encodeURIComponent(profile)}`, `session-${sessionId}`);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'session.jsonl'), '');
}

test('rollup aggregates usage by backend, model, and UTC day', () => {
  const stateDir = fixture();
  try {
    sessionLog(stateDir, 'repo', 'a');
    appendEvents('repo', [
      { type: 'assistant/message', seq: 1, turn: 1, text: 'one', backend: 'codex', model: 'gpt-5.3', usage: { input_tokens: 100, output_tokens: 50, total_tokens: 150, estimated_cost_usd: 0.01, duration_seconds: 1 }, timestamp: NOW },
      { type: 'assistant/message', seq: 2, turn: 2, text: 'two', backend: 'codex', model: 'gpt-5.3', usage: { input_tokens: 30, output_tokens: 20, total_tokens: 50, estimated_cost_usd: 0.005, duration_seconds: 1 }, timestamp: NOW - 1000 },
      // Same backend, different model: separate row.
      { type: 'assistant/message', seq: 3, turn: 3, text: 'three', backend: 'codex', model: 'gpt-5.3-spark', usage: { input_tokens: 10, output_tokens: 5, total_tokens: 15, estimated_cost_usd: 0, duration_seconds: 1 }, timestamp: NOW },
      // Different backend, same day.
      { type: 'assistant/message', seq: 4, turn: 4, text: 'four', backend: 'claude', model: null, usage: { input_tokens: 7, output_tokens: 3, total_tokens: 10, estimated_cost_usd: 0, duration_seconds: 1 }, timestamp: NOW },
      // Yesterday: separate day row, still inside a 7d window.
      { type: 'assistant/message', seq: 5, turn: 5, text: 'five', backend: 'codex', model: 'gpt-5.3', usage: { input_tokens: 500, output_tokens: 500, total_tokens: 1000, estimated_cost_usd: 0.1, duration_seconds: 1 }, timestamp: NOW - DAY }
    ], { stateDir, sessionId: 'a' });

    const rollup = usageRollup('repo', 7, { stateDir, now: () => NOW });
    assert.equal(rollup.rows.length, 4);
    assert.equal(rollup.unattributed_turns, 0);

    const codexToday = rollup.rows.find((row) => row.backend === 'codex' && row.model === 'gpt-5.3' && row.day === '2026-08-30');
    assert.ok(codexToday);
    assert.equal(codexToday.turns, 2);
    assert.equal(codexToday.total_tokens, 200);
    const estimatedCost = codexToday.estimated_cost_usd;
    assert.notEqual(estimatedCost, null);
    assert.ok(Math.abs((estimatedCost ?? 0) - 0.015) < 1e-9);
  } finally {
    setChatSessionStoreOptions({ stateDir: undefined });
    rmSync(stateDir, { recursive: true, force: true });
  }
});

test('rollup keeps named backend accounts separate', () => {
  const stateDir = fixture();
  try {
    sessionLog(stateDir, 'repo', 'accounts');
    appendEvents('repo', [
      { type: 'assistant/message', seq: 1, turn: 1, text: 'work', backend: 'codex', backendInstance: 'codex-work', model: 'gpt-5.3', usage: { input_tokens: 10, output_tokens: 5, total_tokens: 15, estimated_cost_usd: 0.01, duration_seconds: 1 }, timestamp: NOW },
      { type: 'assistant/message', seq: 2, turn: 2, text: 'personal', backend: 'codex', backendInstance: 'codex-personal', model: 'gpt-5.3', usage: { input_tokens: 20, output_tokens: 10, total_tokens: 30, estimated_cost_usd: 0.02, duration_seconds: 1 }, timestamp: NOW }
    ], { stateDir, sessionId: 'accounts' });

    const rows = usageRollup('repo', 7, { stateDir, now: () => NOW }).rows;
    assert.deepEqual(rows.map((row) => [row.backend_instance, row.total_tokens]).sort(), [
      ['codex-personal', 30],
      ['codex-work', 15]
    ]);
  } finally {
    setChatSessionStoreOptions({ stateDir: undefined });
    rmSync(stateDir, { recursive: true, force: true });
  }
});

test('rollup counts usage-less turns as unattributed and honors the window', () => {
  const stateDir = fixture();
  try {
    sessionLog(stateDir, 'repo', 'b');
    appendEvents('repo', [
      { type: 'assistant/message', seq: 1, turn: 1, text: 'no usage', backend: 'agy', model: null, usage: null, timestamp: NOW - 1000 },
      { type: 'assistant/message', seq: 2, turn: 2, text: 'outside window', backend: 'codex', model: null, usage: { input_tokens: 999, output_tokens: 999, total_tokens: 1998, estimated_cost_usd: 1, duration_seconds: 1 }, timestamp: NOW - 8 * DAY }
    ], { stateDir, sessionId: 'b' });

    const week = usageRollup('repo', 7, { stateDir, now: () => NOW });
    assert.equal(week.rows.length, 1, 'the unknown turn remains visible; the old measured turn is outside the window');
    assert.equal(week.rows[0].total_tokens, null);
    assert.equal(week.unattributed_turns, 1);

    const month = usageRollup('repo', 30, { stateDir, now: () => NOW });
    assert.equal(month.rows.length, 2);
    assert.equal(month.unattributed_turns, 1);
  } finally {
    setChatSessionStoreOptions({ stateDir: undefined });
    rmSync(stateDir, { recursive: true, force: true });
  }
});

test('rollup retains measured backend usage alongside unknown backend rows', () => {
  const stateDir = fixture();
  try {
    sessionLog(stateDir, 'repo', 'mixed');
    appendEvents('repo', [
      { type: 'assistant/message', seq: 1, turn: 1, text: 'measured', backend: 'claude', model: 'claude-opus-4-1', usage: { input_tokens: 100, output_tokens: 50, total_tokens: 150, estimated_cost_usd: 0.01, duration_seconds: 1 }, timestamp: NOW - 1000 },
      { type: 'assistant/message', seq: 2, turn: 2, text: 'missing counters', backend: 'codex', model: 'gpt-5.3', usage: null, timestamp: NOW }
    ], { stateDir, sessionId: 'mixed' });

    const rollup = usageRollup('repo', 7, { stateDir, now: () => NOW });
    assert.equal(rollup.rows.length, 2);
    assert.equal(rollup.rows.find(row => row.backend === 'claude')?.total_tokens, 150);
    assert.equal(rollup.rows.find(row => row.backend === 'codex')?.total_tokens, null);
    assert.equal(rollup.tickets[0]?.total_tokens, null);
    assert.deepEqual(rollup.usage_unavailable, [{
      session_id: 'mixed',
      backend: 'codex',
      model: 'gpt-5.3',
      day: '2026-08-30'
    }]);
  } finally {
    setChatSessionStoreOptions({ stateDir: undefined });
    rmSync(stateDir, { recursive: true, force: true });
  }
});

test('rollup preserves unknown cost instead of presenting it as zero', () => {
  const stateDir = fixture();
  try {
    sessionLog(stateDir, 'repo', 'unknown-cost');
    appendEvents('repo', [
      { type: 'assistant/message', seq: 1, turn: 1, text: 'known', backend: 'codex', model: null, usage: { input_tokens: 10, output_tokens: 5, total_tokens: 15, estimated_cost_usd: 0.01, duration_seconds: 1 }, timestamp: NOW },
      { type: 'assistant/message', seq: 2, turn: 2, text: 'unknown', backend: 'codex', model: null, usage: { input_tokens: 10, output_tokens: 5, total_tokens: 15, estimated_cost_usd: null, duration_seconds: 1 }, timestamp: NOW }
    ], { stateDir, sessionId: 'unknown-cost' });

    const rollup = usageRollup('repo', 7, { stateDir, now: () => NOW });
    assert.equal(rollup.rows[0]?.estimated_cost_usd, null);
    assert.equal(rollup.tickets[0]?.estimated_cost_usd, null);
  } finally {
    setChatSessionStoreOptions({ stateDir: undefined });
    rmSync(stateDir, { recursive: true, force: true });
  }
});

test('usage rolls up per ticket via the session index branch (#940 cost-per-ticket)', () => {
  const stateDir = fixture();
  try {
    // A session index so the rollup can join usage -> ticket.
    const profileDir = join(stateDir, 'project-repo');
    mkdirSync(join(profileDir, 'session-issue1'), { recursive: true });
    mkdirSync(join(profileDir, 'session-pr9'), { recursive: true });
    writeFileSync(join(profileDir, 'sessions.json'), JSON.stringify({ sessions: [
      { id: 'issue1', profile: 'repo', worktreePath: null, branch: 'gah/issue/repo-1014-abc', backend: 'codex', model: null, reasoningEffort: null, title: 'Fix the thing', createdAt: NOW, lastActiveAt: NOW, archivedAt: null, outcome: 'live', settledAt: null, settledReason: null },
      { id: 'pr9', profile: 'repo', worktreePath: null, branch: 'feat/my-pr-branch', backend: 'claude', model: null, reasoningEffort: null, title: 'PR chat', createdAt: NOW, lastActiveAt: NOW, archivedAt: null, outcome: 'live', settledAt: null, settledReason: null }
    ] }));
    appendEvents('repo', [
      { type: 'assistant/message', seq: 1, turn: 1, text: 'a', backend: 'codex', model: 'gpt-5.3', usage: { input_tokens: 100, output_tokens: 100, total_tokens: 200, estimated_cost_usd: 0.02, duration_seconds: 1 }, timestamp: NOW },
      { type: 'assistant/message', seq: 2, turn: 2, text: 'b', backend: 'codex', model: 'gpt-5.3', usage: { input_tokens: 50, output_tokens: 50, total_tokens: 100, estimated_cost_usd: 0.01, duration_seconds: 1 }, timestamp: NOW }
    ], { stateDir, sessionId: 'issue1' });
    appendEvents('repo', [
      { type: 'assistant/message', seq: 1, turn: 1, text: 'c', backend: 'claude', model: null, usage: { input_tokens: 10, output_tokens: 10, total_tokens: 20, estimated_cost_usd: 0, duration_seconds: 1 }, timestamp: NOW }
    ], { stateDir, sessionId: 'pr9' });

    const rollup = usageRollup('repo', 7, { stateDir, now: () => NOW });
    assert.equal(rollup.tickets.length, 2);
    const issue = rollup.tickets.find((row) => row.ticket === '#1014');
    assert.ok(issue, 'issue branch maps to #1014');
    assert.equal(issue.title, 'Fix the thing');
    assert.equal(issue.turns, 2);
    assert.equal(issue.total_tokens, 300);
    assert.deepEqual(issue.backends, { codex: 300 });
    const pr = rollup.tickets.find((row) => row.ticket === 'feat/my-pr-branch');
    assert.ok(pr, 'PR chats roll up under their head branch');
    assert.equal(pr.total_tokens, 20);

    assert.equal(ticketLabelForBranch('gah/issue/repo-42'), '#42');
    assert.equal(ticketLabelForBranch('gah/issue/repo-42-mfrgg'), '#42');
    assert.equal(ticketLabelForBranch('gah/issue/git-agent-harness-1014-mfrgg'), '#1014');
    assert.equal(ticketLabelForBranch(null), null);
  } finally {
    setChatSessionStoreOptions({ stateDir: undefined });
    rmSync(stateDir, { recursive: true, force: true });
  }
});
