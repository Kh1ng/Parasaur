import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';

// test.mjs passes --test-force-exit so a test file that leaves a handle open
// cannot hang the suite. These runs go through the real runner against
// fixtures outside src/ that leave a server listening.
const serverRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const fixtures = join(serverRoot, 'tests', 'fixtures', 'test-runner');
const [major, minor] = process.versions.node.split('.').map(Number);
const skip = process.platform === 'win32'
  ? 'process-group cleanup is Unix-only'
  : major >= 22 || (major === 20 && minor >= 14) ? false : 'this Node has no --test-force-exit';

interface Run { code: number | null; timedOut: boolean; output: string }

/** Run node in its own process group and kill the whole group at the
 * deadline, so a hung run cannot leave its test-file process behind. */
function runNode(args: string[], timeoutMs: number, fixtureDir?: string): Promise<Run> {
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT; // this file's own runner sets it
  if (fixtureDir) env.GAH_SERVER_TEST_DIR = fixtureDir;
  return new Promise((resolveRun, reject) => {
    const child = spawn(process.execPath, args, { cwd: serverRoot, env, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '';
    child.stdout.on('data', (chunk) => { output += chunk; });
    child.stderr.on('data', (chunk) => { output += chunk; });
    const killGroup = () => {
      try { process.kill(-child.pid!, 'SIGKILL'); } catch { /* already gone */ }
    };
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; killGroup(); }, timeoutMs);
    child.on('error', reject);
    // 'close' waits for every holder of the pipes, so a leftover test-file
    // process counts as a run that did not finish.
    child.on('close', (code) => {
      clearTimeout(timer);
      killGroup();
      resolveRun({ code, timedOut, output });
    });
  });
}

test('the server test runner exits a passing file that leaves a server listening', { skip, timeout: 30_000 }, async () => {
  const run = await runNode(['test.mjs', '--test-reporter=tap'], 20_000, join(fixtures, 'passing'));
  assert.equal(run.timedOut, false, run.output);
  assert.match(run.output, /^Running 1 server test files$/m);
  assert.match(run.output, /^# pass 1$/m, run.output);
  assert.equal(run.code, 0, run.output);
});

test('the server test runner exits a failing file that leaves a server listening and keeps the failure', { skip, timeout: 30_000 }, async () => {
  const run = await runNode(['test.mjs', '--test-reporter=tap'], 20_000, join(fixtures, 'failing'));
  assert.equal(run.timedOut, false, run.output);
  assert.match(run.output, /^# fail 1$/m, run.output);
  assert.notEqual(run.code, 0, run.output);
});

test('without --test-force-exit the passing fixture never exits', { skip, timeout: 30_000 }, async () => {
  // The runner's invocation minus the flag: the fixture really does leak a
  // handle, so the runs above finish only because of the flag.
  const run = await runNode(
    ['--import', 'tsx', '--test', '--test-concurrency=2', '--test-reporter=tap', join(fixtures, 'passing', 'leaks-server.test.ts')],
    5_000
  );
  // Its test finished; only the listening server kept the run alive.
  assert.match(run.output, /^ok 1 - passes and leaves a server listening$/m, run.output);
  assert.equal(run.timedOut, true, run.output);
});
