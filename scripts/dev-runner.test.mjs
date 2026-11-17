import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const launcher = fileURLToPath(new URL('./dev-runner.js', import.meta.url));

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'parasaur dev runner '));
  t.after(() => {
    assert.equal(dirname(root), resolve(tmpdir()));
    assert.ok(basename(root).startsWith('parasaur dev runner '));
    rmSync(root, { recursive: true, force: true });
  });
  const npmCli = join(root, 'fake npm.cjs');
  writeFileSync(npmCli, `
const fs = require('node:fs');
const path = require('node:path');
const workspace = path.basename(process.cwd());
const root = process.env.RUNNER_FIXTURE;
fs.writeFileSync(path.join(root, workspace + '.json'), JSON.stringify({ args: process.argv.slice(2), http: process.env.VITE_PROXY_TARGET, ws: process.env.VITE_WS_PROXY_TARGET }));
if (!process.env.RUNNER_PAIR) process.exit(17);
const deadline = Date.now() + 3000;
const wait = setInterval(() => {
  if (['server', 'web'].every(name => fs.existsSync(path.join(root, name + '.json')))) {
    clearInterval(wait);
    setTimeout(() => process.exit(workspace === 'server' ? 7 : 0), workspace === 'server' ? 0 : 200);
  } else if (Date.now() > deadline) process.exit(99);
}, 10);
`);
  return {
    run: (args, pair = false) => spawnSync(process.execPath, [launcher, ...args], {
      encoding: 'utf8', timeout: 5000,
      env: { ...process.env, npm_execpath: npmCli, RUNNER_FIXTURE: root, RUNNER_PAIR: pair ? '1' : '' }
    }),
    read: workspace => JSON.parse(readFileSync(join(root, `${workspace}.json`), 'utf8')),
  };
}

for (const port of [3774, 38776]) {
  test(`paired mock launch uses the same ${port} port and preserves the first failure`, t => {
    const f = fixture(t);
    const args = port === 3774 ? [] : ['--port', String(port)];
    const result = f.run(['dev:mock', ...args, '--scenario', 'normal'], true);
    assert.equal(result.error, undefined);
    assert.equal(result.status, 7, result.stderr);
    assert.deepEqual(f.read('server').args, ['run', 'dev:mock', '--', '--port', String(port), '--scenario', 'normal']);
    assert.equal(f.read('web').http, `http://127.0.0.1:${port}`);
    assert.equal(f.read('web').ws, `ws://127.0.0.1:${port}`);
  });
}

test('single launch uses the npm CLI path with spaces, forwards arguments, and returns its failure', t => {
  const f = fixture(t);
  const result = f.run(['dev:web', '--port', '38123']);
  assert.equal(result.error, undefined);
  assert.equal(result.status, 17, result.stderr);
  assert.deepEqual(f.read('web').args, ['run', 'dev', '--', '--port', '38123']);
});

test('an invalid mock port fails before starting either process', t => {
  const f = fixture(t);
  const result = f.run(['dev:mock', '--port', '0']);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /port between 1 and 65535/);
});
