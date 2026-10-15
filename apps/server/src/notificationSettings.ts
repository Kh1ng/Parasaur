import { Router } from 'express';
import rateLimit from 'express-rate-limit';
import type { ActivityEvent, DeliveryReceipt, NotificationSettingsUpdate } from '@git-agent-harness/contracts';
import { runConfigSet, type ConfigSetOptions } from './gahCli.js';
import { sendTestNotification } from './notifyDelivery.js';

/** Issue #1454: the notification module's Settings routes. Mount behind
 * `requireOwner`; both routes write state or reach the network, so both are
 * rate-limited here. */
export type NotificationSettingsDeps = {
  save?: (options: ConfigSetOptions) => Promise<void>;
  test?: () => Promise<DeliveryReceipt | null>;
  /** Records the test result in notification history (central only). */
  record?: (event: ActivityEvent) => void;
  now?: () => number;
};

const CHAT_ID = /^(-?\d{1,20}|@[A-Za-z0-9_]{5,64})$/;

/** Validates the body before any process starts. Returns the CLI options or
 * a caller-facing message. */
export function notificationUpdate(body: unknown): ConfigSetOptions | string {
  if (!body || typeof body !== 'object') return 'Send a JSON object.';
  const { enabled, channel, telegram_chat_id: chatId, credential } = body as Record<string, unknown>;
  if (typeof enabled !== 'boolean') return 'enabled must be true or false.';
  if (channel !== undefined && channel !== 'telegram' && channel !== 'discord') return 'channel must be telegram or discord.';
  if (enabled && channel === undefined) return 'Choose Telegram or Discord to turn notifications on.';
  if (chatId !== undefined && chatId !== null && (typeof chatId !== 'string' || (chatId.trim() !== '' && !CHAT_ID.test(chatId.trim())))) {
    return 'A Telegram chat id is a number such as 123456789 or -1001234567890, or a @channel name.';
  }
  if (credential !== undefined && (typeof credential !== 'string' || credential.trim() === '' || credential.length > 512 || /\s/.test(credential.trim()))) {
    return 'The credential must be one line with no spaces.';
  }
  if (credential !== undefined && channel === undefined) return 'Choose the channel this credential belongs to.';
  return {
    notifications_enabled: enabled,
    notification_channel: channel as NotificationSettingsUpdate['channel'],
    // An empty chat id clears it; the CLI owns that rule.
    telegram_chat_id: typeof chatId === 'string' ? chatId.trim() : undefined,
    notification_credential: typeof credential === 'string' ? credential.trim() : undefined
  };
}

/** The CLI's validation message (fixed text, never the credential), without
 * the spawn wrapper around it. */
function cliMessage(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  return text.match(/Error: (.+)/)?.[1]?.trim().slice(0, 200) ?? 'The settings were not saved.';
}

export function notificationSettingsRouter(deps: NotificationSettingsDeps = {}): Router {
  const router = Router();
  const save = deps.save ?? runConfigSet;
  const test = deps.test ?? (() => sendTestNotification());
  const now = deps.now ?? Date.now;
  router.use(rateLimit({ windowMs: 60_000, limit: 10, standardHeaders: true, legacyHeaders: false }));

  router.put('/', async (req, res) => {
    const options = notificationUpdate(req.body);
    if (typeof options === 'string') return res.status(400).json({ error: 'invalid_notification_settings', message: options });
    try {
      await save(options);
      return res.json({ saved: true });
    } catch (error) {
      return res.status(400).json({ error: 'notification_settings_rejected', message: cliMessage(error) });
    }
  });

  router.post('/test', async (_req, res) => {
    const receipt = await test();
    if (!receipt) return res.status(409).json({ error: 'notifications_off', message: 'Turn notifications on and choose a channel first.' });
    const at = new Date(now()).toISOString();
    deps.record?.({
      id: `notification-test:${now()}`,
      occurredAt: at,
      profile: null,
      kind: 'notification_test',
      severity: receipt.ok ? 'success' : 'error',
      title: receipt.ok ? `Test notification sent to ${receipt.target}` : `Test notification to ${receipt.target} failed`,
      message: receipt.ok ? `${receipt.target} accepted the message.` : receipt.reason ?? 'Delivery failed.',
      // In history with its receipt, but not fanned out to push or APNs.
      notificationMuted: false,
      deliveries: [receipt]
    });
    return res.json({ receipt });
  });
  return router;
}
