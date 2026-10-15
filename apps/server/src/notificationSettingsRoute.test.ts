import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import type { DeliveryReceipt } from '@git-agent-harness/contracts';
import { ActivityFeed } from './activityFeed.js';
import { resetCachedCoordinatorIdentity } from './coordinatorIdentity.js';
import { DEVICE_COOKIE, DeviceAccess } from './deviceAccess.js';
import type { ConfigSetOptions } from './gahCli.js';
import { notificationUpdate } from './notificationSettings.js';
import { RegistryService } from './registryService.js';
import { createServer } from './server.js';

test('notification settings validate before the CLI and keep the credential out of argv', () => {
  assert.equal(typeof notificationUpdate({ enabled: 'yes' }), 'string');
  assert.equal(typeof notificationUpdate({ enabled: true }), 'string', 'on needs a channel');
  assert.equal(typeof notificationUpdate({ enabled: true, channel: 'slack' }), 'string');
  assert.equal(typeof notificationUpdate({ enabled: true, channel: 'telegram', telegram_chat_id: '12; rm' }), 'string');
  assert.equal(typeof notificationUpdate({ enabled: true, channel: 'telegram', credential: 'a b' }), 'string');
  assert.equal(typeof notificationUpdate({ enabled: false, credential: '1:x' }), 'string', 'a credential names its channel');
  assert.deepEqual(notificationUpdate({ enabled: false }), {
    notifications_enabled: false, notification_channel: undefined, telegram_chat_id: undefined, notification_credential: undefined
  });
  assert.deepEqual(notificationUpdate({ enabled: true, channel: 'telegram', telegram_chat_id: ' -1001 ', credential: ' 1:tok ' }), {
    notifications_enabled: true, notification_channel: 'telegram', telegram_chat_id: '-1001', notification_credential: '1:tok'
  });
});

test('the notification routes are owner-only, rate-limited, and record the test receipt in history (#1454)', async () => {
  const root = mkdtempSync(join(tmpdir(), 'gah-notification-route-'));
  const saved = { ...process.env };
  Object.assign(process.env, {
    COORDINATOR_TOKEN: 'notification-route-token', GAH_ALLOW_INSECURE_HTTP: '1',
    GAH_COORDINATOR_IDENTITY_PATH: join(root, 'identity.json')
  });
  resetCachedCoordinatorIdentity();
  const access = new DeviceAccess(join(root, 'devices.json'));
  const feed = new ActivityFeed(join(root, 'activity.jsonl'));
  const writes: ConfigSetOptions[] = [];
  let receipt: DeliveryReceipt | null = null;
  const server = http.createServer(createServer({
    node: { role: 'central', central_url: null }, activityFeed: feed, deviceAccess: access,
    registryService: new RegistryService(join(root, 'registry.json')),
    notificationSettings: { save: async (options) => { writes.push(options); }, test: async () => receipt }
  }));
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
  const origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  const offer = access.create({ id: 'central', name: 'Central', origin });
  const phone = access.redeem(offer.code, 'central', origin, 'Phone').token;
  const remote = { Origin: origin, 'X-Forwarded-For': '198.51.100.7', 'Content-Type': 'application/json' };
  const owner = { ...remote, Authorization: 'Bearer notification-route-token' };
  const call = (path: string, method: string, headers: Record<string, string>, body?: object) =>
    fetch(`${origin}/api/settings/notifications${path}`, { method, headers, body: body && JSON.stringify(body) });
  try {
    assert.equal((await call('', 'PUT', remote, { enabled: false })).status, 401);
    assert.equal((await call('/test', 'POST', { ...remote, Cookie: `${DEVICE_COOKIE}=${phone}` })).status, 403, 'a paired phone is not the owner');
    assert.equal(writes.length, 0);

    const bad = await call('', 'PUT', owner, { enabled: true });
    assert.equal(bad.status, 400);
    assert.equal((await bad.json() as { error: string }).error, 'invalid_notification_settings');
    const ok = await call('', 'PUT', owner, { enabled: true, channel: 'telegram', telegram_chat_id: '42', credential: '1:secret' });
    assert.equal(ok.status, 200);
    assert.ok(!JSON.stringify(await ok.json()).includes('1:secret'), 'the credential is never echoed');
    assert.equal(writes.at(-1)?.notification_credential, '1:secret');

    assert.equal((await call('/test', 'POST', owner)).status, 409, 'an off module has nothing to test');
    receipt = { method: 'channel', target: 'Telegram', ok: false, reason: 'HTTP 401', at: '2026-10-15T00:00:00Z' };
    const tested = await call('/test', 'POST', owner);
    assert.equal(tested.status, 200);
    assert.deepEqual((await tested.json() as { receipt: DeliveryReceipt }).receipt, receipt);
    const [history] = feed.notifications();
    assert.equal(history.kind, 'notification_test');
    assert.equal(history.severity, 'error');
    assert.deepEqual(history.deliveries, [receipt], 'the failed receipt is in notification history');

    // Ten owner requests a minute; four were made above, so the eleventh is refused.
    const statuses: number[] = [];
    for (let attempt = 0; attempt < 7; attempt++) statuses.push((await call('/test', 'POST', owner)).status);
    assert.deepEqual(statuses, [200, 200, 200, 200, 200, 200, 429]);
  } finally {
    await new Promise<void>((done) => server.close(() => done()));
    process.env = saved;
    resetCachedCoordinatorIdentity();
    rmSync(root, { recursive: true, force: true });
  }
});
