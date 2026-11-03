import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { workspaceEnvironment } from './workspaceEnvironment.js';

test('workspace CLI reads use the real profile, stay private, and are not cached across spawns', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-workspace-env-'));
  const binary = join(dir, 'gah');
  const previous = process.env.GAH_BINARY;
  const fixture = (text: string) => writeFileSync(binary, '#!/usr/bin/env node\n' + text, { mode: 0o755 });
  process.env.GAH_BINARY = binary;
  try {
    fixture('process.stdout.write(JSON.stringify({ APP_PROFILE: process.argv.at(-1), APP_VALUE: "first" }));');
    const conversation = { key: 'opaque/not-the-profile', profile: 'actual-project' };
    assert.deepEqual(await workspaceEnvironment(conversation), { APP_PROFILE: 'actual-project', APP_VALUE: 'first' });
    fixture('process.stdout.write(JSON.stringify({ APP_VALUE: "second" }));');
    assert.deepEqual(await workspaceEnvironment(conversation), { APP_VALUE: 'second' });
    for (const script of ['process.stdout.write("{PRIVATE_OUTPUT_CANARY");', 'console.log("[]");', 'console.log(JSON.stringify({ APP_VALUE: false }));',
      'console.error("PRIVATE_ERROR_CANARY"); process.exit(1);']) {
      fixture(script);
      await assert.rejects(workspaceEnvironment(conversation), error => {
        assert.ok(error instanceof Error);
        assert.match(error.message, /Cannot load the workspace environment/);
        assert.doesNotMatch(error.message, /PRIVATE_.*_CANARY/);
        return true;
      });
    }
    fixture('setInterval(() => {}, 1000);');
    const controller = new AbortController();
    const cancelled = workspaceEnvironment(conversation, controller.signal);
    controller.abort();
    await assert.rejects(cancelled, /Cannot load the workspace environment/);
  } finally {
    if (previous === undefined) delete process.env.GAH_BINARY; else process.env.GAH_BINARY = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});
