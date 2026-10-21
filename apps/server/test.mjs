import { readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

// Discover every server source test so adding a directory cannot silently
// remove its coverage from CI. Mock-control-plane tests have their own suite.
function testsIn(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? testsIn(path) : entry.name.endsWith('.test.ts') ? [path] : [];
  });
}

const root = dirname(fileURLToPath(import.meta.url));
const tests = testsIn(join(root, 'src')).sort();
if (tests.length === 0) throw new Error('No server tests found');
console.log(`Running ${tests.length} server test files`);
// A test file whose process keeps a handle open after its tests finish (a
// server connection or timer a failed test never released) otherwise hangs
// the whole run with no output: CI until its 6-hour job limit, and the
// `build:web` prebuild that `gah update` runs indefinitely. Node 20.14 and
// 22 can exit each file once its tests are done; results are unaffected.
const [major, minor] = process.versions.node.split('.').map(Number);
const forceExit = major >= 22 || (major === 20 && minor >= 14) ? ['--test-force-exit'] : [];
const result = spawnSync(process.execPath, ['--import', 'tsx', '--test', '--test-concurrency=2', ...forceExit, ...process.argv.slice(2), ...tests], {
  cwd: root,
  stdio: 'inherit'
});
if (result.error) throw result.error;
process.exit(result.status ?? 1);
