import assert from 'node:assert/strict';
import { test } from 'node:test';
import { EventEmitter } from 'node:events';
import { mkdtempSync, rmSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import type { ChildProcess, spawn } from 'node:child_process';
import { WorkerUpdateService, workerServiceUnit } from './workerUpdate.js';

test('a worker restart belongs to its own PID and invocation in the same allowed unit', () => {
  const invocation = 'a'.repeat(32);
  const properties = `MainPID=287\nId=gah-worker.service\nInvocationID=${'b'.repeat(32)}\n\nMainPID=300\nId=gah-worker@demo.service\nInvocationID=${invocation}\n`;
  assert.equal(workerServiceUnit(properties, 300, invocation), 'gah-worker@demo.service');
  assert.throws(() => workerServiceUnit(properties, 287, invocation));
  assert.throws(() => workerServiceUnit(properties, 300, undefined));
  assert.throws(() => workerServiceUnit(properties.replace('@demo', '-other'), 300, invocation));
});

function withStatePath(fn: (statePath: string) => void | Promise<void>): Promise<void> | void {
  const dir = mkdtempSync(join(tmpdir(), 'gah-worker-update-'));
  const statePath = join(dir, 'worker-update-state.json');
  const saved = process.env.PARASAUR_WORKER_UPDATE_STATE_PATH;
  process.env.PARASAUR_WORKER_UPDATE_STATE_PATH = statePath;
  const restore = () => {
    if (saved === undefined) delete process.env.PARASAUR_WORKER_UPDATE_STATE_PATH;
    else process.env.PARASAUR_WORKER_UPDATE_STATE_PATH = saved;
    rmSync(dir, { recursive: true, force: true });
  };
  try {
    const result = fn(statePath);
    if (result && typeof (result as Promise<void>).then === 'function') {
      return (result as Promise<void>).finally(restore);
    }
    restore();
  } catch (error) {
    restore();
    throw error;
  }
}

function fakeChildProcess(pid: number): ChildProcess {
  const child = new EventEmitter() as unknown as {
    pid: number;
    unref: () => void;
    stdout: EventEmitter;
    stderr: EventEmitter;
  };
  child.pid = pid;
  child.unref = () => {};
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  return child as unknown as ChildProcess;
}

test('start with active claims arms the update instead of launching it (issue #1416)', async () => {
  await withStatePath(async (statePath) => {
    const children: ChildProcess[] = [];
    const spawnFn = (() => {
      const child = fakeChildProcess(5151);
      children.push(child);
      return child;
    }) as unknown as typeof spawn;
    let claims = 2;
    const service = new WorkerUpdateService({
      spawnFn,
      countActiveClaims: async () => claims
    });

    const result = await service.start();
    assert.equal(result.started, true);
    assert.equal(result.status.status, 'waiting');
    assert.equal(result.status.active_dispatches, 2);
    assert.equal(children.length, 0, 'a mid-dispatch worker must not launch yet');

    // The state file survives on disk for central to poll.
    const onDisk = JSON.parse(readFileSync(statePath, 'utf8')) as { status: string };
    assert.equal(onDisk.status, 'waiting');
  });
});

test('unreadable claims arm the update instead of launching it', async () => {
  await withStatePath(async () => {
    let spawned = 0;
    const spawnFn = (() => { spawned += 1; return fakeChildProcess(5152); }) as unknown as typeof spawn;
    const service = new WorkerUpdateService({
      spawnFn,
      countActiveClaims: async () => { throw new Error('gah claims list failed'); }
    });

    const result = await service.start();
    assert.equal(result.status.status, 'waiting');
    assert.equal(result.status.active_dispatches, null);
    assert.equal(spawned, 0, 'a worker whose claims cannot be read must not launch');
    service.cancel();
  });
});

test('start with no claims launches the updater immediately and records its outcome', async () => {
  await withStatePath(async () => {
    const child = fakeChildProcess(process.pid);
    const spawnFn = (() => child) as unknown as typeof spawn;
    const service = new WorkerUpdateService({
      spawnFn,
      countActiveClaims: async () => 0
    });

    const result = await service.start();
    assert.equal(result.started, true);
    assert.equal(service.status().status, 'running');

    (child.stdout as unknown as EventEmitter).emit('data', 'downloading...\n');
    (child as unknown as EventEmitter).emit('close', 0);

    const final = service.status();
    assert.equal(final.status, 'success');
    assert.equal(final.output, 'downloading...\n');
    assert.equal(final.active_dispatches, null);
  });
});

test('a failed updater exit is recorded as failed', async () => {
  await withStatePath(async () => {
    const child = fakeChildProcess(5353);
    const spawnFn = (() => child) as unknown as typeof spawn;
    const service = new WorkerUpdateService({
      spawnFn,
      countActiveClaims: async () => 0
    });
    await service.start();
    (child as unknown as EventEmitter).emit('close', 1);
    assert.equal(service.status().status, 'failed');
  });
});

test('two concurrent update requests launch one installer', async () => {
  await withStatePath(async () => {
    let launches = 0;
    const child = fakeChildProcess(process.pid);
    const service = new WorkerUpdateService({
      countActiveClaims: async () => 0,
      spawnFn:(() => { launches++; return child; }) as unknown as typeof spawn
    });
    const results = await Promise.all([service.start(),service.start()]);
    assert.deepEqual(results.map(result => result.started), [true,false]);
    assert.equal(launches,1);
    (child as unknown as EventEmitter).emit('close',0);
  });
});

test('a second start is refused while the updater is alive, and cancel only disarms waiting', async () => {
  await withStatePath(async () => {
    const child = fakeChildProcess(process.pid);
    const spawnFn = (() => child) as unknown as typeof spawn;
    let claims = 2;
    const service = new WorkerUpdateService({
      spawnFn,
      countActiveClaims: async () => claims
    });

    // Waiting: cancel works.
    await service.start();
    assert.equal(service.status().status, 'waiting');
    assert.equal(service.cancel(), true);
    assert.equal(service.status().status, 'idle');

    // Running: cancel refuses, second start refuses.
    claims = 0;
    await service.start();
    assert.equal(service.status().status, 'running');
    assert.equal(service.cancel(), false);
    const second = await service.start();
    assert.equal(second.started, false);
    assert.equal(second.status.status, 'running');

    (child as unknown as EventEmitter).emit('close', 0);
  });
});

test('a draining worker retains the selected manifest when the channel moves', async t => {
  await withStatePath(async path => {
    t.mock.timers.enable({ apis: ['setInterval'] });
    const env = process.env.PARASAUR_RELEASE_MANIFEST;
    const feed = join(dirname(path), 'edge.json');
    process.env.PARASAUR_RELEASE_MANIFEST = feed;
    const expected = 'a'.repeat(40);
    const platform = process.platform === 'darwin' ? 'macos' : process.arch === 'arm64' ? 'linux-aarch64' : 'linux-x86_64';
    const release = (commit: string) => JSON.stringify({ schema: 1, channel: 'edge', version: '0.3.0', commit,
      assets: [['cli', 'gah-' + platform], ['mcp-server', 'mcp-' + platform], ['server-bundle', 'bundle']].map(([kind, name]) => ({
        name, kind, sha256: '1'.repeat(64), url: name
      })) });
    const children: ChildProcess[] = [];
    let claims = 1;
    let selected: string | undefined;
    const service = new WorkerUpdateService({ countActiveClaims: async () => claims, spawnFn: ((_bin, args: string[]) => {
      const path = args[args.indexOf('--release-manifest') + 1];
      selected = JSON.parse(readFileSync(path, 'utf8')).commit;
      const child = fakeChildProcess(process.pid);
      children.push(child);
      return child;
    }) as typeof spawn });
    try {
      writeFileSync(feed, release(expected));
      await assert.rejects(service.start('c'.repeat(40)), /published build changed/);
      assert.equal(service.status().status, 'idle');
      const armed = await service.start(expected);
      assert.equal(armed.status.status, 'waiting');
      assert.equal(armed.status.target_revision, expected);
      assert.equal('selectedManifest' in armed.status, false);
      writeFileSync(feed, release('b'.repeat(40)));
      claims = 0;
      t.mock.timers.tick(15_000);
      await new Promise<void>(done => setImmediate(done));
      assert.equal(selected, expected);
      assert.equal(service.status().status, 'running');
      // Admission beat the poll. The installer exits before any file changes.
      children[0].emit('close', 75);
      assert.equal(service.status().status, 'waiting');
      assert.equal(service.draining(), true);
      selected = undefined;
      t.mock.timers.tick(15_000);
      await new Promise<void>(done => setImmediate(done));
      assert.equal(selected, expected);
      assert.equal(service.status().status, 'running');
      assert.equal(children.length, 2);
      children[1].emit('close', 0);
      assert.equal(service.draining(), false);
    } finally {
      if (env === undefined) delete process.env.PARASAUR_RELEASE_MANIFEST;
      else process.env.PARASAUR_RELEASE_MANIFEST = env;
    }
  });
});

test('a running record whose pid died is reconciled to failed', async () => {
  await withStatePath((statePath) => {
    const deadPid = 999_999_999; // far past any real pid.
    writeFileSync(
      statePath,
      JSON.stringify({
        status: 'running',
        current_version: '0.1.3',
        target_version: null,
        armed_at: null,
        started_at: '2026-01-01T00:00:00.000Z',
        finished_at: null,
        active_dispatches: null,
        output: 'updating...',
        pid: deadPid
      })
    );
    const service = new WorkerUpdateService({ countActiveClaims: async () => 0 });
    const status = service.status();
    assert.equal(status.status, 'failed');
    assert.ok(status.finished_at);
    // Reconciled on disk, not just in memory.
    const onDisk = JSON.parse(readFileSync(statePath, 'utf8')) as { status: string };
    assert.equal(onDisk.status, 'failed');
  });
});

test('a restarted worker confirms the selected release and reports a missed restart', async t => {
  await withStatePath(statePath => {
    const bundle = 'a'.repeat(64);
    const previousInvocation = 'b'.repeat(32);
    const now = Date.now();
    t.mock.method(Date,'now',() => now);
    const before = process.env.INVOCATION_ID, beforeRelease = process.env.PARASAUR_WORKER_RELEASE;
    const release = dirname(dirname(process.execPath));
    const service = new WorkerUpdateService({cwd:release});
    const pending = (requestedAt:number) => writeFileSync(statePath,JSON.stringify({
      status:'inferred_restart', restart:{previousInvocation,bundle,requestedAt}
    }));
    try {
      process.env.INVOCATION_ID = previousInvocation;
      process.env.PARASAUR_WORKER_RELEASE = bundle;
      pending(now);
      assert.equal(service.status().status,'inferred_restart');
      pending(now - 61_000);
      assert.equal(service.status().status,'failed');
      process.env.INVOCATION_ID = 'c'.repeat(32);
      pending(now);
      process.env.PARASAUR_WORKER_RELEASE = 'd'.repeat(64);
      assert.equal(service.status().status,'failed');
      if (join(release,'bin/node') === process.execPath) {
        process.env.PARASAUR_WORKER_RELEASE = bundle;
        pending(now);
        assert.equal(service.status().status,'success');
      }
    } finally {
      if (before === undefined) delete process.env.INVOCATION_ID; else process.env.INVOCATION_ID = before;
      if (beforeRelease === undefined) delete process.env.PARASAUR_WORKER_RELEASE; else process.env.PARASAUR_WORKER_RELEASE = beforeRelease;
    }
  });
});

test('the updater runs gah update --from-release --role worker pinned to this checkout', async () => {
  await withStatePath(async () => {
    let capturedArgs: string[] = [];
    let capturedCwd = '';
    const child = fakeChildProcess(5555);
    const spawnFn = ((_bin: string, args: string[], options: { cwd: string }) => {
      capturedArgs = args;
      capturedCwd = options.cwd;
      return child;
    }) as unknown as typeof spawn;
    const service = new WorkerUpdateService({
      spawnFn,
      countActiveClaims: async () => 0,
      cwd: '/opt/gah-checkout'
    });
    await service.start();
    assert.deepEqual(capturedArgs, [
      'update',
      '--from-release',
      '--role',
      'worker',
      '--repo',
      '/opt/gah-checkout'
    ]);
    assert.equal(capturedCwd, '/opt/gah-checkout');
    (child as unknown as EventEmitter).emit('close', 0);
  });
});
