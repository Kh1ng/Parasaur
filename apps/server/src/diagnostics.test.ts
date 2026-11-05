import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { createServer as createHttpServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { gunzipSync, gzipSync } from 'node:zlib';
import { spawnSync } from 'node:child_process';
import { buildDiagnosticsBundle, parseProcesses, parseWindowsProcesses, takeSample } from './diagnostics.js';
import { createServer } from './server.js';

test('keeps the largest processes and every server descendant, without arguments', () => {
  const ps = [
    '    1     0   9000  0.0 /sbin/launchd',
    '  100     1  50000  1.0 /Applications/Parasaur.app/Contents/MacOS/parasaur',
    '  200     1   4000  0.5 node',
    '  300   200    100  0.0 /usr/local/bin/claude',
    '  400   300     50 12.5 opencode',
    '  500     1     10  0.0 sleep'
  ].join('\n');
  const result = parseProcesses(ps, 200, 2);
  assert.deepEqual(result.map((p) => p.pid), [100, 1, 200, 300, 400]);
  assert.equal(result[0].cmd, 'parasaur');
  assert.equal(result.at(-1)!.cpu, 12.5);
});

test('Windows snapshots retain descendants, convert bytes and preserve unknown CPU without command lines', () => {
  const rows = [
    { ProcessId: 1, ParentProcessId: 0, WorkingSetSize: '9216000', Name: 'System' },
    { ProcessId: 100, ParentProcessId: 1, WorkingSetSize: 51200000, Name: 'C:\\apps\\gah-desktop.exe' },
    { ProcessId: 200, ParentProcessId: 1, WorkingSetSize: '4096000', Name: 'node.exe', CommandLine: 'synthetic-secret' },
    { ProcessId: 300, ParentProcessId: 200, WorkingSetSize: 102400, Name: 'claude.exe' },
    { ProcessId: 400, ParentProcessId: 300, WorkingSetSize: 51200, Name: 'agent.exe' },
    { ProcessId: 500, ParentProcessId: 1, WorkingSetSize: 10240, Name: 'other.exe' },
    { ProcessId: 600, ParentProcessId: 1, WorkingSetSize: null, Name: 'unavailable.exe' },
    { ProcessId: 601, ParentProcessId: 1, WorkingSetSize: '-1', Name: 'invalid.exe' },
    null,
  ];
  const result = parseWindowsProcesses(JSON.stringify(rows), 200, 2);
  assert.deepEqual(result.map((p) => p.pid), [100, 1, 200, 300, 400]);
  assert.equal(result[0].cmd, 'gah-desktop.exe');
  assert.equal(result[0].rss_kb, 50_000);
  assert.ok(result.every((p) => p.cpu === null));
  assert.ok(!JSON.stringify(result).includes('synthetic-secret'));
  assert.equal(parseWindowsProcesses(JSON.stringify(rows[2]), 200)[0].rss_kb, 4_000);
  assert.deepEqual(parseWindowsProcesses('null', 200), []);
  assert.throws(() => parseWindowsProcesses('invalid', 200));
});

test('native Windows diagnostics include the sampling process with a real working set', { skip: process.platform !== 'win32' }, async () => {
  const sample = await takeSample();
  assert.equal(sample.processes_status, 'available');
  const server = sample.processes.find((p) => p.pid === process.pid);
  assert.ok(server && server.rss_kb > 0);
  assert.equal(server.cpu, null);
  assert.deepEqual(Object.keys(server).sort(), ['cmd', 'cpu', 'pid', 'ppid', 'rss_kb']);
});

test('a missing process collector is unavailable while server memory remains measured', () => {
  const script = `import { takeSample } from ${JSON.stringify(new URL('./diagnostics.js', import.meta.url).href)};
    const sample = await takeSample();
    console.log(JSON.stringify({ status: sample.processes_status, processes: sample.processes, rss: sample.server.rss_bytes }));`;
  const child = spawnSync(process.execPath, ['--import', import.meta.resolve('tsx'), '--input-type=module', '-e', script], {
    cwd: mkdtempSync(join(tmpdir(), 'gah-diag-no-collector-')),
    env: { ...process.env, PATH: '' }, encoding: 'utf8', timeout: 10_000,
  });
  assert.equal(child.status, 0, child.stderr);
  const sample = JSON.parse(child.stdout);
  assert.equal(sample.status, 'unavailable');
  assert.deepEqual(sample.processes, []);
  assert.ok(sample.rss > 0);
});

test('bundle carries the rotated and current timeline and skips torn lines', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-diag-'));
  writeFileSync(join(dir, 'memory.1.jsonl'), '{"t":"a"}\n');
  writeFileSync(join(dir, 'memory.jsonl'), '{"t":"b"}\n{"t":');
  const bundle = JSON.parse(gunzipSync(await buildDiagnosticsBundle('worker', dir)).toString());
  assert.equal(bundle.format, 'parasaur-diagnostics/1');
  assert.equal(bundle.node_role, 'worker');
  assert.deepEqual(bundle.timeline.map((s: { t: string }) => s.t), ['a', 'b']);
  assert.ok(bundle.current.server.rss_bytes > 0);
});

test('export rejects past the rate limit before building a bundle', async () => {
  let builds = 0;
  const app = createServer({
    node: { role: 'worker', central_url: 'https://central.test' },
    buildDiagnosticsBundle: async () => { builds++; return gzipSync('{}'); }
  });
  const server = createHttpServer(app);
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
  const { port } = server.address() as AddressInfo;
  try {
    const statuses: number[] = [];
    for (let i = 0; i < 12; i++) statuses.push((await fetch(`http://127.0.0.1:${port}/api/diagnostics/export`)).status);
    assert.deepEqual(statuses, [...Array(10).fill(200), 429, 429]);
    assert.equal(builds, 10);
  } finally {
    await new Promise<void>((done) => server.close(() => done()));
  }
});
