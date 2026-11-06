import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import { WebSocket } from 'ws';

test('the real entrypoint serves health and info while status is held, reports failure, then recovers', { skip: process.platform === 'win32' ? 'POSIX executable fixture' : false, timeout: 20000 }, async () => {
  const root = mkdtempSync(join(tmpdir(), 'gah-startup-'));
  const cli = join(root, 'gah');
  const entered = join(root, 'status-entered');
  const gate = join(root, 'status-mode');
  const snapshot = JSON.parse(readFileSync(new URL('../tests/fixtures/gah/responses/status.json', import.meta.url), 'utf8'));
  writeFileSync(cli, `#!${process.execPath}
const fs=require('node:fs');const args=process.argv.slice(2);
if(args[0]==='--version'){console.log('gah 0.3.0');process.exit(0)}
if(args[0]==='status'&&args.includes('--role')){console.log(JSON.stringify({role:'central',central_url:null}));process.exit(0)}
if(args[0]==='status'){
 fs.writeFileSync(${JSON.stringify(entered)},'entered');
 const timer=setInterval(()=>{if(!fs.existsSync(${JSON.stringify(gate)}))return;
  clearInterval(timer);if(fs.readFileSync(${JSON.stringify(gate)},'utf8')==='fail'){console.error('fixture status unavailable');process.exit(1)}
  console.log(${JSON.stringify(JSON.stringify(snapshot))});process.exit(0);
 },20);
}else{console.log('{}')}
`);
  chmodSync(cli, 0o755);
  const reservation = http.createServer();
  await new Promise<void>(resolve => reservation.listen(0, '127.0.0.1', resolve));
  const port = (reservation.address() as AddressInfo).port;
  await new Promise<void>(resolve => reservation.close(() => resolve()));
  const child = spawn(process.execPath, ['--import', import.meta.resolve('tsx'), fileURLToPath(new URL('./bin.ts', import.meta.url))], {
    cwd: root, detached: true, stdio: ['ignore', 'pipe', 'pipe'],
    env: { PATH: process.env.PATH, HOME: root, XDG_CONFIG_HOME: join(root, 'config'), HOST: '127.0.0.1', PORT: String(port),
      GAH_BINARY: cli, GAH_WEB_ROOT: '', GAH_DISABLE_PRICE_REFRESH: '1', COORDINATOR_TOKEN: 'startup-fixture-owner' }
  });
  let output = '';
  child.stdout.on('data', data => { output = (output + data).slice(-20000); });
  child.stderr.on('data', data => { output = (output + data).slice(-20000); });
  const closed = new Promise<void>(resolve => child.once('close', () => resolve()));
  const origin = `http://127.0.0.1:${port}`;
  const health = async () => {
    const response = await fetch(origin+'/health', { signal: AbortSignal.timeout(500) });
    assert.equal(response.status, 200);
    return response.json() as Promise<{ status: string; checks: Record<string, { ready: boolean; error?: string }> }>;
  };
  const until = async (check: () => Promise<boolean>, message: string) => {
    const deadline = Date.now()+6000;
    while (true) {
      try { if (await check()) return; } catch { /* Listener may not exist yet. */ }
      assert.equal(child.exitCode, null, output);
      assert.ok(Date.now()<deadline, message+'\n'+output);
      await new Promise(resolve => setTimeout(resolve, 20));
    }
  };
  let socket: WebSocket | undefined;
  try {
    await until(async () => existsSync(entered) && (await health()).checks.providerRegistry.ready === false, 'Listener must answer while status remains blocked');
    assert.equal(existsSync(gate), false);
    assert.equal((await health()).status, 'starting');
    const info = await fetch(origin+'/api/info', { headers: { Authorization: 'Bearer startup-fixture-owner' } });
    assert.equal(info.status, 200);
    writeFileSync(gate, 'fail');
    await until(async () => (await health()).checks.providerRegistry.error === 'Provider status refresh failed', 'A failed initial read must remain visible');
    writeFileSync(gate, 'ok');
    socket = new WebSocket(origin.replace('http:', 'ws:')+'/ws', { headers: { Authorization: 'Bearer startup-fixture-owner' } });
    await new Promise<void>((resolve, reject) => { socket!.once('open', resolve); socket!.once('error', reject); });
    socket.send(JSON.stringify({ type: 'provider.refresh', requestId: 'recover', instanceId: 'codex_instance_0' }));
    await until(async () => (await health()).checks.providerRegistry.ready === true, 'A later provider refresh must recover readiness');
    assert.equal((await health()).status, 'healthy');
  } finally {
    socket?.terminate();
    if (child.exitCode === null && child.pid) { try { process.kill(-child.pid, 'SIGKILL'); } catch { /* Already exited. */ } }
    await closed;
    rmSync(root, { recursive: true });
  }
});
