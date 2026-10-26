import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { invalidateAgentModels, resolveAdapter, resolveInstanceAdapter } from './managerChat/registry.js';

test('model adapter observes direct terminal updates once and ignores failed updates', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-model-update-'));
  const previous = process.env.XDG_STATE_HOME;
  const routerSettings = process.env.GAH_CLI_ROUTER_SETTINGS_PATH;
  process.env.GAH_CLI_ROUTER_SETTINGS_PATH = join(dir, 'unconfigured-router.json');
  process.env.XDG_STATE_HOME = dir;
  const adapter = resolveAdapter('codex');
  const invalidate = adapter.invalidateModels;
  let invalidations = 0;
  adapter.invalidateModels = () => { invalidations++; };
  try {
    const folder = join(dir, 'gah/agent-cli-updates'); mkdirSync(folder, { recursive: true });
    const path = join(folder, 'codex.json');
    writeFileSync(path, JSON.stringify({ backend: 'codex', status: 'success', started_at: 'first', finished_at: 'first-finished' }));
    const refreshed = resolveAdapter('codex');
    assert.equal(refreshed, adapter);
    assert.equal(resolveAdapter('codex'), refreshed);
    assert.equal(invalidations, 1);
    writeFileSync(path, JSON.stringify({ backend: 'codex', status: 'failed', started_at: 'second', finished_at: 'second-finished' }));
    assert.equal(resolveAdapter('codex'), refreshed); assert.equal(invalidations, 1);
    writeFileSync(path, JSON.stringify({ backend: 'codex', status: 'success', started_at: 'third', finished_at: 'third-finished' }));
    assert.equal(resolveAdapter('codex'), refreshed);
    assert.equal(invalidations, 2);
    writeFileSync(path, JSON.stringify({ backend: 'codex', status: 'success', started_at: 'fourth' }));
    assert.equal(resolveAdapter('codex'), refreshed);
    assert.equal(resolveAdapter('codex'), refreshed);
    assert.equal(invalidations, 3, 'missing finished_at uses the same revision for comparison and storage');
  } finally {
    adapter.invalidateModels = invalidate;
    if (previous === undefined) delete process.env.XDG_STATE_HOME; else process.env.XDG_STATE_HOME = previous;
    if (routerSettings === undefined) delete process.env.GAH_CLI_ROUTER_SETTINGS_PATH; else process.env.GAH_CLI_ROUTER_SETTINGS_PATH = routerSettings;
    rmSync(dir, { recursive: true, force: true });
  }
});


test('CLI updates invalidate named accounts by runner kind, including logical aliases', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-alias-model-update-'));
  const previous = process.env.GAH_BINARY;
  const stateHome = process.env.XDG_STATE_HOME;
  const routerSettings = process.env.GAH_CLI_ROUTER_SETTINGS_PATH;
  process.env.XDG_STATE_HOME = dir;
  process.env.GAH_CLI_ROUTER_SETTINGS_PATH = join(dir, 'unconfigured-router.json');
  try {
    const runtime = { backend_instance: 'work', runner_kind: 'claude', logical_backend: 'team-claude',
      executable: '/tools/claude', state_root: '/isolated/work', account_label: 'Work',
      credential_id: 'work-key', credential_provider: 'anthropic', credential_revision: 'generation-1' };
    const cli = join(dir, 'gah');
    writeFileSync(cli, `#!/bin/sh\nprintf '%s' '${JSON.stringify(runtime)}'\n`, { mode: 0o700 });
    process.env.GAH_BINARY = cli;
    const first = await resolveInstanceAdapter('agent-update-test', 'team-claude', 'work');
    assert.equal(await resolveInstanceAdapter('agent-update-test', 'team-claude', 'work'), first);
    let refreshes = 0;
    first.invalidateModels = () => { refreshes++; };
    invalidateAgentModels('claude');
    assert.equal(await resolveInstanceAdapter('agent-update-test', 'team-claude', 'work'), first);
    assert.equal(refreshes, 1);
  } finally {
    invalidateAgentModels('claude');
    if (previous === undefined) delete process.env.GAH_BINARY; else process.env.GAH_BINARY = previous;
    if (stateHome === undefined) delete process.env.XDG_STATE_HOME; else process.env.XDG_STATE_HOME = stateHome;
    if (routerSettings === undefined) delete process.env.GAH_CLI_ROUTER_SETTINGS_PATH; else process.env.GAH_CLI_ROUTER_SETTINGS_PATH = routerSettings;
    rmSync(dir, { recursive: true, force: true });
  }
});
