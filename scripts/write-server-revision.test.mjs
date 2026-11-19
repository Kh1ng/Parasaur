import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

test('the server records the checked-out build revision only for identifiable source', t => {
  const root = mkdtempSync(join(tmpdir(), 'parasaur-build-stamp-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const dir of ['scripts', 'apps/server/dist', 'apps/server/src']) {
    mkdirSync(join(root, dir), { recursive: true });
  }
  copyFileSync(new URL('./write-server-revision.mjs', import.meta.url), join(root, 'scripts/write-server-revision.mjs'));
  const stamp = () => {
    execFileSync(process.execPath, ['scripts/write-server-revision.mjs'], {
      cwd: root, env: { ...process.env, GITHUB_SHA: 'f'.repeat(40) }
    });
    return JSON.parse(readFileSync(join(root, 'apps/server/dist/build-revision.json'), 'utf8')).revision;
  };
  assert.equal(stamp(), null);
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
  git('init', '-q');
  writeFileSync(join(root, 'apps/server/src/bin.ts'), 'export {};\n');
  git('add', 'scripts', 'apps/server/src');
  git('-c', 'user.name=Build test', '-c', 'user.email=build@example.invalid', 'commit', '-qm', 'Build source');
  assert.equal(stamp(), git('rev-parse', 'HEAD'));
  git('config', 'status.showUntrackedFiles', 'no');
  writeFileSync(join(root, 'apps/server/src/new.ts'), 'export const changed = true;\n');
  assert.equal(stamp(), null);
});
