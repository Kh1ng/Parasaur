import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join, resolve, sep } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const installer = readFileSync(new URL('./install-windows.ps1', import.meta.url), 'utf8').replaceAll('\r\n', '\n');
const startup = installer.match(/\$startupScript = @'\n([\s\S]*?)\n'@/)?.[1];
assert(startup?.includes('sleep infinity') && startup.includes('start gah-worker.service'));

for (const exitCode of [0, -1]) {
  test(`an exited WSL keepalive (${exitCode}) fails the task so its retry policy applies`, { skip: process.platform !== 'win32' }, t => {
    const root = mkdtempSync(join(tmpdir(), 'parasaur-worker-supervisor-'));
    t.after(() => {
      assert(resolve(root).startsWith(resolve(tmpdir()) + sep));
      assert(basename(root).startsWith('parasaur-worker-supervisor-'));
      rmSync(root, { recursive: true, force: true });
    });
    const script = join(root, 'supervisor.ps1');
    const mocks = `
function global:wsl.exe {
    $global:LASTEXITCODE = 0
    if ($args -contains 'hostname') { '100.98.69.105' }
    if ($args -contains 'sleep') { $global:LASTEXITCODE = ${exitCode} }
}
function global:netsh.exe { $global:LASTEXITCODE = 0 }
`;
    const settings = JSON.stringify({ distribution: 'Fixture', worker_ip: '100.98.69.105', central_ip: '100.118.97.79' });
    writeFileSync(script, mocks + startup.replace('__SETTINGS__', settings));
    const result = spawnSync(join(process.env.SystemRoot, 'System32/WindowsPowerShell/v1.0/powershell.exe'),
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script],
      { encoding: 'utf8', timeout: 30_000, windowsHide: true });
    assert.ifError(result.error);
    assert.notEqual(result.status, 0, 'Task Scheduler must not see a completed worker supervisor as success');
    assert.match(result.stderr, /WSL worker keepalive exited/);
  });
}
