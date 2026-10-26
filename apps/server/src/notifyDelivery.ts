import { spawn } from 'node:child_process';
import { activityPath, notifiableActivity, type ActivityEvent, type DeliveryMethod, type DeliveryReceipt } from '@git-agent-harness/contracts';
import { findGahBinary } from './gahCli.js';
import type { ActivityDeliverer } from './activityFeed.js';

export type ActivityDelivery = ActivityDeliverer;

/** A receipt reason short enough for a chip: the HTTP status when one is
 * named, otherwise the first line of the error, bounded. */
export function receiptReason(error: unknown): string {
  const text = error instanceof Error ? error.message : String(error);
  return text.match(/\bHTTP \d{3}\b/)?.[0] ?? text.split('\n')[0].trim().slice(0, 60);
}

export function deliveryReceipt(method: DeliveryMethod, target: string, failure?: unknown): DeliveryReceipt {
  return {
    method,
    target,
    ok: failure === undefined,
    ...(failure === undefined ? {} : { reason: receiptReason(failure) }),
    at: new Date().toISOString()
  };
}

/** Controller events are read back from the Rust event log. Rust already
 * delivered their channel message when it recorded them (and a worker's log
 * never reaches this feed), so the channel only takes events Node originates. */
function fromControllerLog(event: ActivityEvent): boolean {
  return event.origin === 'controller';
}

function activityLine(event: ActivityEvent, baseUrl: string): string {
  const url = new URL(activityPath(event), baseUrl).href;
  return `[gah] ${event.title}: ${event.message} ${url}`.replace(/\s+/g, ' ').trim();
}

class ExitError extends Error {
  constructor(label: string, readonly code: number | null, readonly stdout: string, stderr: string) {
    super(`${label} exited ${code}: ${stderr.trim()}`);
  }
}

function run(label: string, command: string, args: string[], stdin?: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: [stdin === undefined ? 'ignore' : 'pipe', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout?.on('data', (chunk) => { stdout = (stdout + chunk).slice(-2_000); });
    child.stderr?.on('data', (chunk) => { stderr = (stderr + chunk).slice(-500); });
    child.on('error', reject);
    child.on('close', (code) => code === 0 ? resolve(stdout) : reject(new ExitError(label, code, stdout, stderr)));
    // A hook may exit without reading its input; its exit code decides the outcome.
    child.stdin?.on('error', () => undefined);
    child.stdin?.end(stdin);
  });
}

const CHANNEL_NAMES: Record<string, string> = { telegram: 'Telegram', discord: 'Discord' };
let channelFailure: DeliveryReceipt | null = null;

/** A rejected Telegram configuration stays quiet until Settings saves or tests it. */
export function notificationChannelFailure(): DeliveryReceipt | null { return channelFailure; }
export function resetNotificationChannelFailure(): void { channelFailure = null; }

/** `gah notify-send` prints one JSON line naming the channel it used (`none`
 * while the module is off) and, on failure, a redacted `reason`. An older
 * CLI prints nothing; its success is then unknown and records no receipt. */
function channelReceipt(stdout: string, failure?: unknown): DeliveryReceipt[] {
  let line: { channel?: unknown; reason?: unknown } = {};
  try { line = JSON.parse(stdout.trim().split('\n').at(-1) ?? '') as typeof line; } catch { /* Older CLI. */ }
  const { channel, reason } = line;
  if (channel === 'none' || (channel === undefined && failure === undefined)) return [];
  const target = typeof channel === 'string' ? CHANNEL_NAMES[channel] ?? channel : 'Channel';
  const why = typeof reason === 'string' && reason ? reason : failure;
  return [deliveryReceipt('channel', target, failure === undefined ? undefined : why)];
}

/** One `gah notify-send`. Resolves to its receipt, or none when the module is
 * off (the CLI then exits 0 and prints channel "none", so nothing is logged). */
async function sendThroughChannel(gahBinary: string, args: string[], label: string): Promise<DeliveryReceipt[]> {
  try {
    const receipts = channelReceipt(await run('gah notify-send', gahBinary, ['notify-send', ...args]));
    channelFailure = null;
    return receipts;
  } catch (error) {
    const receipts = channelReceipt(error instanceof ExitError ? error.stdout : '', error);
    const failure = receipts.find(receipt => receipt.target === 'Telegram' && receipt.reason === 'HTTP 400');
    if (failure) channelFailure = failure;
    console.error(`[activity] channel delivery failed for ${label}: ${error instanceof Error ? error.message : String(error)}`);
    return receipts;
  }
}

/** Telegram/Discord through `gah notify-send`, reusing the Rust channel code.
 * The CLI no-ops when the module is off or no channel is configured. */
export function channelDelivery(baseUrl: string, gahBinary: () => string = findGahBinary): ActivityDelivery {
  let pending = Promise.resolve();
  return (event) => {
    if (!notifiableActivity(event) || fromControllerLog(event)) return [];
    // Serialize admission so concurrent events cannot all pass the first HTTP 400.
    const delivery = pending.then(() => channelFailure ? [] : sendThroughChannel(gahBinary(), [
      '--title', event.title,
      '--message', event.message,
      '--url', new URL(activityPath(event), baseUrl).href
    ], event.id));
    pending = delivery.then(() => undefined, () => undefined);
    return delivery;
  };
}

/** Issue #1454: the Settings test message. `null` means the module is off. */
export async function sendTestNotification(gahBinary: () => string = findGahBinary): Promise<DeliveryReceipt | null> {
  const [receipt] = await sendThroughChannel(gahBinary(), [
    '--title', 'Test notification',
    '--message', 'Sent from Parasaur Settings. Notifications reach you here.'
  ], 'the test notification');
  return receipt ?? null;
}

/** One shell hook for every notifiable event, piped as a single line on stdin
 * (the same shape as the Rust per-profile `notify_command`). */
export function commandDelivery(baseUrl: string, env: NodeJS.ProcessEnv = process.env): ActivityDelivery | undefined {
  let command = env.GAH_NOTIFY_COMMAND;
  if (!command && env.GAH_NODE_LIVENESS_NOTIFY_COMMAND) {
    console.log('GAH_NODE_LIVENESS_NOTIFY_COMMAND is deprecated; rename it to GAH_NOTIFY_COMMAND. It now receives every notifiable event.');
    command = env.GAH_NODE_LIVENESS_NOTIFY_COMMAND;
  }
  if (!command) return undefined;
  const hook = command;
  return async (event) => {
    if (!notifiableActivity(event)) return [];
    try {
      await run('GAH_NOTIFY_COMMAND', 'sh', ['-c', hook], `${activityLine(event, baseUrl)}\n`);
      return [deliveryReceipt('command', 'Command hook')];
    } catch (error) {
      console.error(`[activity] command hook failed for ${event.id}: ${error instanceof Error ? error.message : String(error)}`);
      return [deliveryReceipt('command', 'Command hook', error instanceof ExitError ? `exit ${error.code}` : error)];
    }
  };
}

/** Fan one recorded event out to every delivery method and collect their
 * receipts. One failing method never blocks the others. */
export function deliverToAll(deliveries: (ActivityDelivery | undefined)[]): (event: ActivityEvent) => Promise<DeliveryReceipt[]> {
  const active = deliveries.filter((delivery): delivery is ActivityDelivery => !!delivery);
  return async (event) => {
    const results = await Promise.allSettled(active.map(async (delivery) => await delivery(event)));
    return results.flatMap((result) => {
      if (result.status === 'fulfilled') return result.value ?? [];
      console.error(`[activity] delivery failed for ${event.id}: ${result.reason instanceof Error ? result.reason.message : String(result.reason)}`);
      return [];
    });
  };
}
