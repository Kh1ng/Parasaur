import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import test from 'node:test';

test('streamed session output does not remain in the server heap', t => {
  const source = `
    import { randomBytes } from 'node:crypto';
    const { createSessionManager } = await import(${JSON.stringify(new URL('./SessionManager.js', import.meta.url).href)});
    let delivered = 0;
    const manager = createSessionManager({ disableCleanupTimer: true,
      providerRegistry: { isProviderAvailable: () => true },
      pushBus: { publish(message) { if (message.type === 'session.stdout' || message.type === 'session.stderr') delivered += message.data.length; } },
      dispatchRunner: () => ({ promise: new Promise(() => {}), cancel: async () => ({ cancelled: true }) })
    });
    const session = await manager.startSession({ profile: 'memory-test', providerKind: 'github', instanceId: 'github-0', repo: 'fixture/repo', mode: 'fix' });
    await new Promise(resolve => setImmediate(resolve));
    global.gc();
    const before = process.memoryUsage().heapUsed;
    for (let i = 0; i < 10000; i++) manager.addSessionOutput(session.id, randomBytes(2048).toString('hex'), i % 2 === 0);
    global.gc();
    const growth = process.memoryUsage().heapUsed - before;
    console.log(JSON.stringify({ delivered, growth, status: manager.getSession(session.id).status }));
  `;
  const output = execFileSync(process.execPath, ['--expose-gc', '--import', 'tsx', '--input-type=module', '-e', source], {
    encoding: 'utf8', timeout: 20000, maxBuffer: 64 * 1024, windowsHide: true
  });
  const result = JSON.parse(output.trim());
  t.diagnostic(`Delivered ${result.delivered} characters; post-GC retained heap growth ${result.growth} bytes.`);
  assert.ok(result.delivered >= 10000 * 4096, 'every output character is delivered live');
  assert.equal(result.status, 'running', 'a noisy session is not cancelled to bound memory');
  assert.ok(result.growth < 8 * 1024 * 1024, `40 MiB of streamed output retained ${result.growth} heap bytes`);
});
