import { test, expect } from '@playwright/experimental-ct-react';
import React from 'react';
import type { NotificationSettingsSummary } from '@git-agent-harness/contracts';
import { NotificationChannelSection } from '../../src/pages/SettingsPage.js';

// Issue #1454: Telegram/Discord notifications are one optional module.

const configWith = (notifications: NotificationSettingsSummary) => ({
  data: { current_manager: null, notifications },
  loading: false,
  error: null,
});
const noop = async () => {};

test('a fresh install is off, asks for nothing, and turns on in one save with token and chat id', async ({ mount, page }) => {
  const saved: unknown[] = [];
  await page.route('**/api/settings/notifications', async (route) => {
    saved.push(route.request().postDataJSON());
    await route.fulfill({ json: { saved: true } });
  });
  const component = await mount(
    <NotificationChannelSection
      config={configWith({ enabled: false, channel: 'none', telegram_chat_id: null, credential_env: null, credential_source: null })}
      setConfig={noop}
      clearConfigErrors={() => {}}
    />
  );
  const toggle = component.getByRole('checkbox', { name: /Send notifications to Telegram or Discord/ });
  await expect(toggle).not.toBeChecked();
  await expect(component.getByLabel('Bot token')).toHaveCount(0);
  await expect(component.getByRole('combobox', { name: 'Channel' })).toHaveCount(0);
  await expect(component.getByRole('button', { name: 'Send test message' })).toHaveCount(0);

  await toggle.check();
  await expect(component.getByRole('combobox', { name: 'Channel' })).toHaveValue('telegram');
  await expect(component.getByText('message @BotFather', { exact: false })).toBeVisible();
  const save = component.getByRole('button', { name: 'Save notifications' });
  await expect(save).toBeDisabled();
  await component.getByLabel('Bot token').fill('123456:secret');
  await component.getByRole('textbox', { name: 'Telegram chat ID' }).fill('4242');
  await save.click();
  await expect(component.getByRole('status')).toContainText('Telegram notifications are on');
  expect(saved).toEqual([{ enabled: true, channel: 'telegram', telegram_chat_id: '4242', credential: '123456:secret' }]);
  await expect(component.getByLabel('Bot token')).toHaveValue('');
});

test('an enabled module sends a test message and shows a failed receipt; off saves only the switch', async ({ mount, page }) => {
  const saved: unknown[] = [];
  await page.route('**/api/settings/notifications', async (route) => {
    saved.push(route.request().postDataJSON());
    await route.fulfill({ json: { saved: true } });
  });
  await page.route('**/api/settings/notifications/test', (route) => route.fulfill({
    json: { receipt: { method: 'channel', target: 'Discord', ok: false, reason: 'HTTP 404', at: '2026-10-15T00:00:00Z' } },
  }));
  const component = await mount(
    <NotificationChannelSection
      config={configWith({ enabled: true, channel: 'discord', telegram_chat_id: null, credential_env: 'DISCORD_WEBHOOK_URL', credential_source: 'file' })}
      setConfig={noop}
      clearConfigErrors={() => {}}
    />
  );
  await expect(component.getByText('Saved on this host (private file).', { exact: false })).toBeVisible();
  await component.getByRole('button', { name: 'Send test message' }).click();
  await expect(component.getByRole('status')).toContainText('The test message to Discord failed: HTTP 404. It is recorded in notification history.');

  await component.getByRole('checkbox', { name: /Send notifications to Telegram or Discord/ }).uncheck();
  await expect(component.getByLabel('Webhook URL')).toHaveCount(0);
  await component.getByRole('button', { name: 'Save notifications' }).click();
  await expect(component.getByRole('status')).toContainText('Notifications are off');
  expect(saved).toEqual([{ enabled: false }]);
});


test('a rejected Telegram channel appears once in Settings with a recovery action', async ({ mount }) => {
  const component = await mount(
    <NotificationChannelSection
      config={configWith({ enabled: true, channel: 'telegram', telegram_chat_id: '42', credential_env: 'TELEGRAM_BOT_TOKEN', credential_source: 'file', last_error: 'Telegram notification rejected with HTTP 400' })}
      setConfig={noop}
      clearConfigErrors={() => {}}
    />
  );
  await expect(component.getByRole('alert')).toHaveCount(1);
  await expect(component.getByRole('alert')).toContainText('Telegram delivery paused');
  await expect(component.getByRole('alert')).toContainText('HTTP 400');
  await expect(component.getByRole('button', { name: 'Send test message' })).toBeEnabled();
});
