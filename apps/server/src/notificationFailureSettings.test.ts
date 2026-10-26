import assert from 'node:assert/strict';
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import type { NotificationSettingsSummary } from '@git-agent-harness/contracts';
import { withFixtureServer } from './fixtureGahHarness.js';
import { channelDelivery, resetNotificationChannelFailure } from './notifyDelivery.js';

test('Settings config exposes the channel rejection and saving clears it', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'gah-channel-settings-'));
  const gah = join(directory, 'gah');
  writeFileSync(gah, `#!/bin/sh
if [ "$1" = notify-send ]; then
  echo '{"channel":"telegram","ok":false,"reason":"Telegram notification rejected with HTTP 400"}'
  exit 1
fi
if [ "$1" = config ] && [ "$2" = show ]; then
  echo '{"current_manager":null,"notifications":{"channel":"telegram","telegram_chat_id":"42","credential_env":"TELEGRAM_BOT_TOKEN"}}'
fi
`);
  chmodSync(gah, 0o755);
  resetNotificationChannelFailure();
  try {
    await channelDelivery('https://central.example', () => gah)({ id: 'failed', profile: 'gah', kind: 'chat_turn_failed', occurredAt: new Date().toISOString(), title: 'Chat failed', message: 'ACP connection closed', severity: 'error' });
    await withFixtureServer(async (baseUrl) => {
      process.env.GAH_BINARY = gah;
      const config = async () => (await (await fetch(`${baseUrl}/api/config`)).json()) as { notifications: NotificationSettingsSummary };
      assert.match((await config()).notifications.last_error ?? '', /HTTP 400/);
      const saved = await fetch(`${baseUrl}/api/settings/notifications`, {
        method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ enabled: false })
      });
      assert.equal(saved.status, 200);
      assert.equal((await config()).notifications.last_error, null);
    });
  } finally {
    resetNotificationChannelFailure();
    rmSync(directory, { recursive: true, force: true });
  }
});
