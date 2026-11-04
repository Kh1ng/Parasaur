import { expect, test, type Page, type WebSocketRoute } from '@playwright/test';
import { readFileSync } from 'node:fs';
import type { ClientMessage } from '@git-agent-harness/contracts';

const fixture = JSON.parse(readFileSync(new URL('../../../server/tests/fixtures/gah/responses/status.json', import.meta.url), 'utf8'));
type Start = Extract<ClientMessage, { type: 'session.start' }>;

async function setup(page: Page, supported = true) {
  const starts: Start[] = [];
  let socket: WebSocketRoute;
  const account = (id: string) => ({ backend_instance: id, logical_backend: 'codex', runner_kind: 'codex',
    account_label: `${id} login`, quota_pool: id, supported_models: ['chosen-model'], enabled: id !== 'disabled',
    executable_configured: true, auth_ready: true });
  const nodes = ['worker-one', 'worker-two'].map(node_id => ({ node_id, display_name: node_id,
    profile: 'fixture', profiles: ['fixture'], state: 'healthy', dispatch_backend_instances: true, backend_configured: { codex: true },
    backend_instances: [account(node_id === 'worker-one' ? 'work' : 'personal'), account('disabled')] }));
  await page.route('**/api/status**', route => route.fulfill({ json: { ...fixture, node_id: 'central', display_name: 'Central',
    dispatch_backend_instances: supported, backend_instances: [account('central-account')],
    nodes, backend_configured: { codex: false }, available_tickets: [], blocked_work_items: [], review_held_work_ids: [] } }));
  await page.route('**/api/manager-chat/settings', route => route.fulfill({ json: { defaultBackend: 'codex', profileOverrides: {},
    availableBackends: [{ id: 'codex', displayName: 'Codex', implemented: true }] } }));
  await page.routeWebSocket('**/ws**', ws => {
    socket = ws;
    ws.send(JSON.stringify({ type: 'server.welcome', serverVersion: 'test', profile: 'fixture',
      serverProviderCatalog: { providers: [] }, sessions: [], providers: {}, backendConfigured: { codex: true } }));
    ws.onMessage(raw => { const message = JSON.parse(String(raw)); if (message.type === 'session.start') starts.push(message); });
  });
  await page.goto('/?profile=fixture');
  await page.getByRole('button', { name: 'Factory', exact: true }).click();
  const form = page.getByRole('region', { name: 'Queue work', exact: true });
  await form.getByRole('button', { name: 'Factory backend' }).click();
  await page.getByRole('dialog', { name: 'Factory backend' }).getByRole('button', { name: 'Codex', exact: true }).click();
  return { form, starts, reply: (message: unknown) => socket.send(JSON.stringify(message)) };
}

test('factory pins account and model, waits for acknowledgement and retries the identical uncertain request', async ({ page }, testInfo) => {
  await page.clock.install();
  const { form, starts, reply } = await setup(page);
  await form.getByLabel('Execution node').selectOption('worker-one');
  await form.getByLabel('Account', { exact: true }).selectOption('work');
  await form.getByLabel('Model (optional)').fill('chosen-model');
  await form.getByLabel('Target (issue number or ticket path)').fill('1680');
  await page.screenshot({ path: testInfo.outputPath('factory-account-desktop.png') });
  await page.setViewportSize({ width: 390, height: 844 });
  expect(await form.evaluate(element => element.scrollWidth <= element.clientWidth)).toBe(true);
  for (const label of ['Execution node', 'Account', 'Model (optional)']) {
    expect((await form.getByLabel(label, { exact: true }).boundingBox())!.height).toBeGreaterThanOrEqual(44);
  }
  await form.screenshot({ path: testInfo.outputPath('factory-account-mobile.png') });
  // Plain HTTP LAN browsers expose getRandomValues but no randomUUID.
  await page.evaluate(() => Object.defineProperty(crypto, 'randomUUID', { value: undefined, configurable: true }));
  await form.getByRole('button', { name: 'Dispatch', exact: true }).click();
  await expect.poll(() => starts.length).toBe(1);
  expect(starts[0]).toMatchObject({ backend: 'codex', backendInstance: 'work', nodeId: 'worker-one', model: 'chosen-model', target: '1680' });
  await expect(form.getByRole('button', { name: 'Awaiting confirmation…' })).toBeDisabled();
  await expect(form.getByLabel('Execution node')).toBeDisabled();
  await expect(form.getByText(/Dispatch confirmed/)).toHaveCount(0);
  await page.clock.fastForward(15_001);
  await expect(form.getByText(/Work may have started/)).toBeVisible();
  await form.getByRole('button', { name: 'Retry same request' }).click();
  await expect.poll(() => starts.length).toBe(2);
  expect(starts[1]).toEqual(starts[0]);
  // Joining existing work returns the older session's requestId. The direct
  // response still acknowledges this operator's request and must clear pending.
  reply({ type: 'session.started', requestId: starts[0]!.requestId, session: { ...starts[0],
    id: 'account-session', requestId: 'older-request', status: 'running', startedAt: new Date().toISOString() } });
  await expect(form.getByRole('status')).toContainText('Dispatch confirmed: account-session');
  await expect(form.getByLabel('Execution node')).toBeEnabled();
});

test('node changes clear account and model; admission errors remain visible', async ({ page }) => {
  const { form, starts, reply } = await setup(page);
  await form.getByLabel('Execution node').selectOption('worker-one');
  await form.getByLabel('Account', { exact: true }).selectOption('work');
  await form.getByLabel('Model (optional)').fill('chosen-model');
  await form.getByLabel('Execution node').selectOption('worker-two');
  await expect(form.getByLabel('Account', { exact: true })).toHaveValue('');
  await expect(form.getByLabel('Account', { exact: true }).locator('option[value="work"]')).toHaveCount(0);
  await expect(form.getByLabel('Model (optional)')).toHaveValue('');
  await expect(form.getByLabel('Account', { exact: true }).locator('option[value="disabled"]')).toHaveJSProperty('disabled', true);
  await form.getByLabel('Account', { exact: true }).selectOption('personal');
  await form.getByRole('button', { name: 'Dispatch', exact: true }).click();
  await expect.poll(() => starts.length).toBe(1);
  reply({ type: 'error', requestId: starts[0]!.requestId, error: 'Selected account requires login on worker-two.' });
  await expect(form.getByRole('alert')).toContainText('Selected account requires login');
  await expect(form.getByRole('button', { name: 'Dispatch', exact: true })).toBeEnabled();
  await form.getByLabel('Execution node').selectOption('central');
  await expect(form.getByLabel('Account', { exact: true }).locator('option[value="central-account"]')).toHaveCount(1);
});

test('an older central disables exact accounts while retaining ordinary dispatch', async ({ page }) => {
  const { form, starts } = await setup(page, false);
  await form.getByLabel('Execution node').selectOption('worker-one');
  await expect(form.getByLabel('Account', { exact: true })).toBeDisabled();
  await expect(form.getByText('Update central to select named factory accounts.')).toBeVisible();
  await form.getByRole('button', { name: 'Dispatch', exact: true }).click();
  await expect.poll(() => starts.length).toBe(1);
  expect(starts[0]!.backendInstance).toBeUndefined();
});

test('sample factory dispatch reaches the stateful socket server and returns its account receipt', async ({ page, request }) => {
  const base = process.env.GAH_MOCK_BASE_URL!;
  await request.post(`${base}/api/mock/reset`);
  try {
    await page.goto('/?profile=fixture');
    await page.getByRole('button', { name: 'Factory', exact: true }).click();
    const form = page.getByRole('region', { name: 'Queue work', exact: true });
    await form.getByRole('button', { name: 'Factory backend' }).click();
    await page.getByRole('dialog', { name: 'Factory backend' }).getByRole('button', { name: 'Codex', exact: true }).click();
    await form.getByLabel('Execution node').selectOption('mock-central');
    await form.getByLabel('Account', { exact: true }).selectOption('codex-work');
    await form.getByLabel('Model (optional)').fill('gpt-5.3-codex');
    await form.getByLabel('Target (issue number or ticket path)').fill('1680');
    await form.getByRole('button', { name: 'Dispatch', exact: true }).click();
    await expect(form.getByRole('status')).toContainText('Dispatch confirmed: mock-worker-1');
    await expect(page.getByRole('button').filter({ hasText: 'codex / codex-work' })).toContainText('gpt-5.3-codex');
  } finally { await request.post(`${base}/api/mock/reset`); }
});
