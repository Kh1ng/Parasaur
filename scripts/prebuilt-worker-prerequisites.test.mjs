import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const source = readFileSync(new URL('./install-wsl-worker.sh', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
// Execute the real prerequisite block, stopping before any installation path,
// download, config write or service activation can be reached.
const preflight = source.slice(0, source.indexOf('install_dir='));
assert(preflight.includes('missing_packages=()') && !preflight.includes('release_dir='));

function check(t, missing = '') {
  const root = mkdtempSync(join(tmpdir(), 'gah-worker-prerequisites-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const executable = (name, text) => writeFileSync(join(root, name), `#!/bin/sh\n${text}\n`, { mode: 0o755 });
  executable('id', 'echo 1000');
  executable('uname', 'echo x86_64');
  executable('systemctl', 'exit 0');
  executable('dpkg-query', '[ "$3" = "build-essential" ] || [ "$3" = "$MISSING_PACKAGE" ] || { echo installed; exit 0; }; echo not-installed');
  executable('sudo', 'printf "%s\\n" "$*" >> "$SUDO_LOG"');
  const log = join(root, 'sudo.log');
  const result = spawnSync('/bin/bash', ['-c', preflight, 'worker-preflight', root], {
    encoding: 'utf8', env: { PATH: `${root}:/usr/bin:/bin`, HOME: root, MISSING_PACKAGE: missing, SUDO_LOG: log },
  });
  assert.equal(result.status, 0, result.stderr);
  return existsSync(log) ? readFileSync(log, 'utf8') : '';
}

test('a prebuilt worker needs no compiler package or sudo when runtime prerequisites exist', { skip: process.platform === 'win32' }, t => {
  assert.equal(check(t), '');
});

test('a missing runtime prerequisite installs only that prerequisite', { skip: process.platform === 'win32' }, t => {
  assert.equal(check(t, 'curl'), 'apt-get update\napt-get install -y curl\n');
});
