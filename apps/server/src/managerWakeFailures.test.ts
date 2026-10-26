import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { test } from 'node:test';
import { ActivityFeed, activityFromChat } from './activityFeed.js';
import { deliverToAll } from './notifyDelivery.js';
import { ManagerWakeQueue } from './managerChat/managerWake.js';
import { resolveAdapter } from './managerChat/registry.js';
import { sendManagerChatMessage, setChatEventPublishers, setSessionLogOptions } from './managerChat/ManagerChatManager.js';

test('20 consecutive wake failures produce one activity and one channel delivery', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'gah-wake-failures-'));
  const saved = { ...process.env };
  Object.assign(process.env, {
    GAH_BINARY: resolve('tests/fixtures/gah/gah'),
    GAH_GATEWAY_SETTINGS_PATH: join(directory, 'gateway.json'),
    GAH_MANAGER_CHAT_SETTINGS_PATH: join(directory, 'settings.json')
  });
  delete process.env.TDAI_GATEWAY_URL;
  delete process.env.TDAI_GATEWAY_API_KEY;
  setSessionLogOptions({ stateDir: directory });
  const adapter = resolveAdapter('hermes');
  const original = adapter.runTurn;
  let failures = 20;
  adapter.runTurn = async () => {
    if (failures-- > 0) throw new Error('ACP connection closed');
    return { reply: 'OK', model: null, usage: null };
  };
  const delays: number[] = [];
  const wakes = new ManagerWakeQueue(async (ms) => { delays.push(ms); });
  const delivered: string[] = [];
  const retryStarts: boolean[] = [];
  const feed = new ActivityFeed(null, deliverToAll([async (event) => {
    if (event.kind === 'chat_turn_failed') delivered.push(event.id);
    return [];
  }]));
  setChatEventPublishers({ lifecycle: (event) => {
    if (event.phase === 'start') retryStarts.push(event.wakeRetry ?? false);
    const activity = activityFromChat(event);
    if (activity) feed.record(activity);
  } });
  try {
    await wakes.enqueue('wake-failures', retryCount => sendManagerChatMessage('wake-failures', '/status', 'wake-1', undefined, 'hermes', undefined, { wakeRetry: retryCount > 0 }));
    await new Promise((done) => setImmediate(done));
    assert.equal(feed.replay('wake-failures').filter(event => event.kind === 'chat_turn_failed').length, 1);
    assert.equal(delivered.length, 1);
    assert.equal(feed.notifications().find(event => event.kind === 'chat_turn_failed')?.retryCount, 19);
    assert.deepEqual(delays, [5_000, 10_000, 20_000, 40_000, 80_000, 160_000, ...Array<number>(14).fill(300_000)]);
    assert.deepEqual(retryStarts, [false, ...Array<boolean>(20).fill(true)]);
    failures = 1;
    await wakes.enqueue('wake-failures', () => sendManagerChatMessage('wake-failures', '/status', 'wake-2', undefined, 'hermes'));
    await new Promise((done) => setImmediate(done));
    assert.equal(delivered.length, 2, 'failure after success announces again');
    assert.equal(delays.at(-1), 5_000, 'success resets backoff');
  } finally {
    adapter.runTurn = original;
    setChatEventPublishers({});
    setSessionLogOptions({ stateDir: undefined });
    process.env = saved;
    rmSync(directory, { recursive: true, force: true });
  }
});
