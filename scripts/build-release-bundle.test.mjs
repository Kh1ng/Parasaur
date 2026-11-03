import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, unlinkSync, chmodSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const script = readFileSync(new URL('./build-release-bundle.sh', import.meta.url), 'utf8');

/** The script roots itself at `dirname $0/..`, so the fixture copies it into
 * a fake tree; every expected output becomes a one-byte file. */
function fixture(t, { complete = true } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'gah-release-bundle-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, 'scripts'), { recursive: true });
  writeFileSync(join(root, 'scripts/build-release-bundle.sh'), script, { mode: 0o755 });
  for (const name of ['stage-release-runtime.mjs', 'link-release-runtime.mjs']) {
    writeFileSync(join(root, 'scripts', name), readFileSync(new URL(name, import.meta.url)));
  }
  const outputs = [
    'apps/server/dist/bin.js',
    'apps/web/dist/index.html',
    'packages/contracts/dist/index.js',
    'packages/shared/dist/index.js',
    'packages/contracts/package.json',
    'packages/shared/package.json',
    'packaging/opencode/agents/gah-reviewer.md',
    'packaging/opencode/agents/gah-implementer.md',
    'package.json',
    'package-lock.json'
  ];
  for (const path of outputs) {
    if (path === 'package.json' && complete) {
      writeFileSync(join(root, path), '{"version":"0.1.3"}');
      continue;
    }
    if (path === 'package-lock.json' && complete) {
      writeFileSync(join(root, path), JSON.stringify({ lockfileVersion: 3, packages: { '': {}, 'packages/contracts': {}, 'packages/shared': {} } }));
      continue;
    }
    if (complete || path !== 'apps/web/dist/index.html') {
      mkdirSync(join(root, join(path, '..')), { recursive: true });
      writeFileSync(join(root, path), 'fixture');
    }
  }
  const out = join(root, 'gah-server-bundle.tar.gz');
  const run = () => spawnSync('bash', [join(root, 'scripts/build-release-bundle.sh'), out], { encoding: 'utf8' });
  return { root, out, run };
}

test('a complete build archives exactly the release install layout', t => {
  const { run, out } = fixture(t);
  const result = run();
  assert.equal(result.status, 0, result.stderr);
  const listing = spawnSync('tar', ['-tzf', out], { encoding: 'utf8' });
  const names = listing.stdout.trim().split('\n');
  for (const member of [
    'apps/server/dist/bin.js',
    'apps/web/dist/index.html',
    'apps/server/dist/node_modules/@git-agent-harness/contracts/dist/index.js',
    'apps/server/dist/node_modules/@git-agent-harness/shared/dist/index.js',
    'apps/server/dist/node_modules/@git-agent-harness/contracts/package.json',
    'apps/server/dist/node_modules/@git-agent-harness/shared/package.json',
    'apps/server/dist/.release-runtime/package-lock.json',
    'apps/server/dist/.release-runtime/packages/contracts/dist/index.js',
    'apps/server/dist/.release-runtime/packages/shared/dist/index.js',
    'apps/server/dist/.release-runtime/link.mjs',
    'packaging/opencode/agents/gah-reviewer.md',
    'package.json',
    'package-lock.json'
  ]) {
    assert.ok(names.includes(member), `${member} missing from bundle: ${names.join(', ')}`);
  }
  // Server dist carries the whole directory, not just bin.js.
  assert.ok(names.some((name) => name.startsWith('apps/web/dist/')), 'web dist directory missing');
});

test('an absent workspace runtime fails before an archive is written', t => {
  const { root, out, run } = fixture(t);
  unlinkSync(join(root, 'packages/shared/dist/index.js'));
  const result = run();
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /missing build output: packages\/shared\/dist\/index.js/);
  assert.throws(() => readFileSync(out), { code: 'ENOENT' });
});

test('workspace metadata cannot escape release staging', t => {
  const { root, out, run } = fixture(t);
  const lockPath = join(root, 'package-lock.json');
  const lock = JSON.parse(readFileSync(lockPath, 'utf8'));
  lock.packages['../escape'] = {};
  writeFileSync(lockPath, JSON.stringify(lock));
  const result = run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /Invalid workspace path/);
  assert.throws(() => readFileSync(out), { code: 'ENOENT' });
});

test('archive assets are service-readable without changing restrictive build outputs', t => {
  const { root, out, run } = fixture(t);
  const asset = join(root, 'apps/web/dist/index.html');
  chmodSync(asset, 0o600);
  assert.equal(run().status, 0);
  const extracted = join(root, 'extracted');
  mkdirSync(extracted);
  const result = spawnSync('tar', ['-xzf', out, '-C', extracted], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(statSync(join(extracted, 'apps/web/dist/index.html')).mode & 0o777, 0o644);
  assert.equal(statSync(asset).mode & 0o777, 0o600, 'the build tree is untouched');
});

test('the archived server resolves fresh internal packages without a built workspace', t => {
  const { root, out, run } = fixture(t);
  mkdirSync(join(root, 'apps/server/dist/.release-runtime/node_modules/old'), { recursive: true });
  writeFileSync(join(root, 'apps/server/dist/.release-runtime/node_modules/old/keep-out'), 'stale dependencies');
  for (const name of ['contracts', 'shared']) {
    writeFileSync(join(root, `packages/${name}/package.json`), JSON.stringify({ name: `@git-agent-harness/${name}`, type: 'module', main: './dist/index.js' }));
    writeFileSync(join(root, `packages/${name}/dist/index.js`), `export const tag = 'fresh-${name}';`);
    const stale = join(root, `apps/server/dist/node_modules/@git-agent-harness/${name}/dist`);
    mkdirSync(stale, { recursive: true });
    writeFileSync(join(stale, 'index.js'), 'stale-runtime');
  }
  assert.equal(run().status, 0);
  const extracted = join(root, 'cold');
  mkdirSync(extracted);
  assert.equal(spawnSync('tar', ['-xzf', out, '-C', extracted]).status, 0);
  assert.throws(() => readFileSync(join(extracted, 'apps/server/dist/.release-runtime/node_modules/old/keep-out')), { code: 'ENOENT' });
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
    import { tag as contracts } from '@git-agent-harness/contracts';
    import { tag as shared } from '@git-agent-harness/shared';
    if (contracts !== 'fresh-contracts' || shared !== 'fresh-shared') process.exit(1);
  `], { cwd: join(extracted, 'apps/server/dist'), encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(join(root, 'apps/server/dist/node_modules/@git-agent-harness/contracts/dist/index.js'), 'utf8'), 'stale-runtime', 'staging does not modify the original build');
});

test('an incomplete build fails naming the first missing output and writes nothing', t => {
  const { run, out } = fixture(t, { complete: false });
  const result = run();
  assert.equal(result.status, 1);
  assert.match(result.stderr, /missing build output: apps\/web\/dist\/index\.html/);
  assert.match(result.stderr, /build:server/);
  assert.equal(result.stderr.includes('package.json') && result.stderr.includes('run:'), false, 'guidance only, no noise');
  assert.ok(!out || result.status !== 0);
});
