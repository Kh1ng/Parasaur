import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync, symlinkSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'gah-runtime-clean-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'scripts'));
  mkdirSync(join(root, 'apps/server/dist/node_modules'), { recursive: true });
  const script = join(root, 'scripts/clean-server-runtime.mjs');
  writeFileSync(script, readFileSync(new URL('./clean-server-runtime.mjs', import.meta.url)));
  return { root, target: join(root, 'apps/server/dist/node_modules/@git-agent-harness'), run: () => spawnSync(process.execPath, [script], { encoding: 'utf8' }) };
}

test('source builds clean embedded runtime imports and preserve other output', t => {
  const { root, target, run } = fixture(t);
  mkdirSync(join(target, 'contracts'), { recursive: true });
  writeFileSync(join(target, 'contracts/index.js'), 'old runtime');
  const server = join(root, 'apps/server/dist/bin.js');
  writeFileSync(server, 'server');
  assert.equal(run().status, 0);
  assert.equal(existsSync(target), false);
  assert.equal(readFileSync(server, 'utf8'), 'server');
  assert.equal(run().status, 0, 'a fresh source build has nothing to clean');
});

test('cleanup rejects a symlink out of the server directory', t => {
  const { root, target, run } = fixture(t);
  const external = join(root, 'external');
  mkdirSync(external);
  writeFileSync(join(external, 'keep'), 'keep');
  symlinkSync(external, target, process.platform === 'win32' ? 'junction' : 'dir');
  const result = run();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Refusing to clean a release runtime outside/);
  assert.equal(readFileSync(join(external, 'keep'), 'utf8'), 'keep');
});

test('source rebuild removes isolated release dependencies and keeps server output', t => {
  const { root, run } = fixture(t);
  const runtime = join(root, 'apps/server/dist/.release-runtime');
  mkdirSync(runtime);
  writeFileSync(join(runtime, 'format.json'), '{"schema":1}');
  writeFileSync(join(root, 'apps/server/dist/node_modules/release-only'), 'old dependency');
  writeFileSync(join(root, 'apps/server/dist/bin.js'), 'compiled server');
  const result = run();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(existsSync(runtime), false);
  assert.equal(existsSync(join(root, 'apps/server/dist/node_modules')), false);
  assert.equal(readFileSync(join(root, 'apps/server/dist/bin.js'), 'utf8'), 'compiled server');
});

test('an escaping isolated runtime prevents any source cleanup', t => {
  const { root, run } = fixture(t);
  const external = join(root, 'external');
  mkdirSync(external);
  writeFileSync(join(external, 'format.json'), '{"schema":1}');
  const marker = join(root, 'apps/server/dist/node_modules/keep');
  writeFileSync(marker, 'keep');
  symlinkSync(external, join(root, 'apps/server/dist/.release-runtime'), process.platform === 'win32' ? 'junction' : 'dir');
  const result = run();
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Refusing to clean a release runtime outside/);
  assert.equal(readFileSync(marker, 'utf8'), 'keep');
  assert.equal(readFileSync(join(external, 'format.json'), 'utf8'), '{"schema":1}');
});
