import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { AgentCliUpdate, AgentCliVersion } from '@git-agent-harness/contracts';
import { AgentCliError, AgentCliVersions, readAgentUpdates } from './agentCliVersions.js';

const node = { node_id: 'local', display_name: 'Local' };
const row: AgentCliVersion = { backend: 'codex', installed: '1.0.0', latest: '1.1.0', outdated: true,
  source: 'npm:@openai/codex', update_command: 'codex update', error: null };
const deps = { probe: async () => [row], readUpdates: () => [], activeClaims: async () => 0,
  enabled: () => true, optedIn: () => false, nodeUpdating: () => false };
const turn = () => new Promise(resolve => setImmediate(resolve));

test('agents probes coalesce and updates are off unless locally opted in', async () => {
  let probes = 0; let updates = 0;
  const service = new AgentCliVersions(node, { ...deps, probe: async () => { probes++; return [row]; }, launch: async () => { updates++; } });
  const [a, b] = await Promise.all([service.snapshot(), service.snapshot()]);
  assert.deepEqual(a, b); assert.equal(probes, 1);
  await service.scheduledUpdate();
  assert.equal(updates, 0);
  const optedIn = new AgentCliVersions(node, { ...deps, optedIn: () => true, launch: async () => { updates++; } });
  await optedIn.scheduledUpdate(); await turn();
  assert.equal(updates, 1);
});

test('agents update gates fail closed and serialize starts across an asynchronous claims check', async () => {
  let starts = 0;
  for (const override of [{ enabled: () => false }, { activeClaims: async () => 1 },
    { activeClaims: async () => { throw new Error('claims unavailable'); } }, { nodeUpdating: () => true }]) {
    const service = new AgentCliVersions(node, { ...deps, ...override, launch: async () => { starts++; } });
    await assert.rejects(service.start('codex'));
  }
  let finish!: () => void;
  const service = new AgentCliVersions(node, { ...deps, launch: () => { starts++; return new Promise<void>(resolve => { finish = resolve; }); } });
  const first = service.start('codex');
  await assert.rejects(service.start('codex'), (e: unknown) => e instanceof AgentCliError && e.code === 'agent_update_busy');
  await first; assert.equal(starts, 1); finish(); await turn();
  await assert.rejects(service.start('codex;sh'), (e: unknown) => e instanceof AgentCliError && e.status === 400);
});

test('successful persisted updates invalidate models and versions once, including after restart', async () => {
  let updates: AgentCliUpdate[] = []; let refreshes = 0; let probes = 0;
  const overrides = { ...deps, readUpdates: () => updates, refreshModels: async (backend: string) => { assert.equal(backend, 'codex'); refreshes++; },
    probe: async () => { probes++; return [row]; } };
  const service = new AgentCliVersions(node, overrides);
  await service.snapshot();
  updates = [{ backend: 'codex', status: 'success', started_at: 'start', finished_at: 'finish', before: '1.0.0', after: '1.1.0', error: null }];
  await service.snapshot(); await service.snapshot();
  assert.equal(refreshes, 1); assert.equal(probes, 2);
  await new AgentCliVersions(node, overrides).scheduledUpdate();
  assert.equal(refreshes, 2);
  updates = [{ ...updates[0], status: 'failed', started_at: 'later', error: 'failed' }];
  await service.snapshot(); assert.equal(refreshes, 2);
});

test('failed updates are observable even if the CLI could not write its ledger', async () => {
  const service = new AgentCliVersions(node, { ...deps, launch: async () => { throw new Error('spawn error'); } });
  await service.start('codex'); await turn();
  assert.match((await service.snapshot()).error!, /update failed/);
});

test('agents ledger preserves failure after a vanished process and rejects malformed state', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-agent-ledger-'));
  const previous = process.env.XDG_STATE_HOME; process.env.XDG_STATE_HOME = dir;
  try {
    const folder = join(dir, 'gah/agent-cli-updates'); mkdirSync(folder, { recursive: true });
    const path = join(folder, 'codex.json');
    writeFileSync(path, JSON.stringify({ backend: 'codex', status: 'running', started_at: 'today', pid: -1 }));
    assert.equal(readAgentUpdates()[0].status, 'failed');
    writeFileSync(path, '{'); assert.throws(readAgentUpdates, /Cannot read/);
  } finally {
    if (previous === undefined) delete process.env.XDG_STATE_HOME; else process.env.XDG_STATE_HOME = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('agents CLI JSON crosses the process boundary into the node inventory', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-agent-process-'));
  const previous = process.env.GAH_BINARY;
  try {
    const path = join(dir, 'gah');
    const backends = ['claude', 'codex', 'opencode', 'vibe', 'agy', 'openhands', 'hermes'];
    const rows = backends.map(backend => ({ ...row, backend }));
    writeFileSync(path, `#!/bin/sh\n[ "$1 $2 $3" = 'agents versions --json' ] || exit 2\nprintf '%s' '${JSON.stringify(rows)}'\n`, { mode: 0o700 });
    process.env.GAH_BINARY = path;
    const service = new AgentCliVersions(node, { readUpdates: () => [] });
    const snapshot = await service.snapshot();
    assert.deepEqual(snapshot.versions, rows);
    assert.equal(snapshot.node_id, node.node_id);
  } finally {
    if (previous === undefined) delete process.env.GAH_BINARY; else process.env.GAH_BINARY = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a failed CLI update does not starve other outdated CLIs on later ticks', async () => {
  const launched: string[] = [];
  const service = new AgentCliVersions(node, { ...deps, optedIn: () => true,
    probe: async () => [row, { ...row, backend: 'claude' }],
    readUpdates: () => [{ backend: 'codex', status: 'failed', started_at: '2026-01-01', finished_at: '2026-01-01', before: '1.0.0', after: null, error: 'installer failed' }],
    launch: async backend => { launched.push(backend); } });
  await service.scheduledUpdate();
  assert.deepEqual(launched, ['claude']);
});
