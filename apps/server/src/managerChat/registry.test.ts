import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { defaultClaudeSpawn, resolveInstanceAdapter } from './registry.js';

test('named adapter cache enforces backend ownership and changes after credential replacement', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-instance-cache-'));
  const cli = join(dir, 'gah');
  const state = join(dir, 'runtime.json');
  const previous = process.env.GAH_BINARY;
  const runtime = { backend_instance: 'hermes-work', runner_kind: 'hermes', logical_backend: 'hermes',
    executable: '/tools/hermes', state_root: '/isolated/work', account_label: 'Work',
    credential_id: 'work-key', credential_provider: 'nous', credential_revision: 'generation-1' };
  writeFileSync(cli, `#!/bin/sh\ncat '${state}'\n`, { mode: 0o755 });
  try {
    process.env.GAH_BINARY = cli;
    writeFileSync(state, JSON.stringify(runtime));
    const first = await resolveInstanceAdapter('credential-test', 'hermes', 'hermes-work');
    assert.equal(await resolveInstanceAdapter('credential-test', 'hermes', 'hermes-work'), first);
    await assert.rejects(resolveInstanceAdapter('credential-test', 'opencode', 'hermes-work'), /does not serve opencode/);
    writeFileSync(state, JSON.stringify({ ...runtime, credential_revision: 'generation-2' }));
    assert.notEqual(await resolveInstanceAdapter('credential-test', 'hermes', 'hermes-work'), first);
  } finally {
    if (previous === undefined) delete process.env.GAH_BINARY; else process.env.GAH_BINARY = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('default Claude adapter runs the installed claude and flags the bundled fallback', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-claude-path-'));
  const bundled = join(dir, 'node_modules', '.bin');
  const notExecutable = join(dir, 'plain');
  const installed = join(dir, 'bin');
  for (const folder of [bundled, notExecutable, installed]) mkdirSync(folder, { recursive: true });
  writeFileSync(join(bundled, 'claude'), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
  writeFileSync(join(notExecutable, 'claude'), '#!/bin/sh\nexit 0\n', { mode: 0o644 });
  writeFileSync(join(installed, 'claude'), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
  try {
    const found = defaultClaudeSpawn(['relative', bundled, notExecutable, installed].join(delimiter));
    assert.equal(found.bundledRuntime, false);
    assert.deepEqual(found.env, { CLAUDE_CODE_EXECUTABLE: join(installed, 'claude') });

    const none = defaultClaudeSpawn([bundled, notExecutable].join(delimiter));
    assert.equal(none.bundledRuntime, true);
    assert.equal(none.env, undefined);
    assert.deepEqual(none.args, found.args);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
