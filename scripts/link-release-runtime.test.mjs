import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, rmSync, writeFileSync, symlinkSync, renameSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

test('release imports preserve hoisted, workspace-local and transitive versions after moving dist', { skip: process.platform === 'win32' }, t => {
  const root = mkdtempSync(join(tmpdir(), 'gah-release-imports-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const dist = join(root, 'staging/dist');
  const runtime = join(dist, '.release-runtime');
  const module = (path, content) => {
    mkdirSync(path, { recursive: true });
    writeFileSync(join(path, 'package.json'), JSON.stringify({ type: 'module', main: 'index.js' }));
    writeFileSync(join(path, 'index.js'), content);
  };
  module(join(runtime, 'node_modules/versioned'), "export default 'hoisted';");
  module(join(runtime, 'apps/server/node_modules/versioned'), "export default 'server-local';");
  module(join(runtime, 'node_modules/transitive'), "export default 'root-transitive';");
  module(join(runtime, 'packages/shared'), "import v from 'versioned'; import t from 'transitive'; export default [v,t];");
  module(join(runtime, 'packages/shared/node_modules/versioned'), "export default 'shared-local';");
  mkdirSync(join(runtime, 'node_modules/@git-agent-harness'));
  symlinkSync('../../packages/shared', join(runtime, 'node_modules/@git-agent-harness/shared'), 'dir');
  cpSync(new URL('./link-release-runtime.mjs', import.meta.url), join(runtime, 'link.mjs'));
  writeFileSync(join(dist, 'check.mjs'), `
    import assert from 'node:assert/strict';
    import v from 'versioned';
    import shared from '@git-agent-harness/shared';
    assert.equal(v, 'server-local');
    assert.deepEqual(shared, ['shared-local', 'root-transitive']);
  `);
  const prepared = spawnSync(process.execPath, [join(runtime, 'link.mjs')], { encoding: 'utf8' });
  assert.equal(prepared.status, 0, prepared.stderr);
  const installed = join(root, 'installed');
  renameSync(dist, installed);
  const result = spawnSync(process.execPath, [join(installed, 'check.mjs')], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
});
