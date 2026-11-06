import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

import ts from 'typescript';
import { WebSocket } from 'ws';
import type { ClientMessage, ProjectImportResult, ServerMessage, PairingAccessRequest, PairedDevice, RecordedUsageReport } from '@git-agent-harness/contracts';
import { createMockControlPlane, MOCK_SCENARIOS } from './controlPlane.js';
import type { ComputeOutputCatalog, ComputeOutputDownload, ComputeJournalReview, NodeFactorySnapshot } from '@git-agent-harness/contracts';

test('sample recorded usage combines chat and factory readings with explicit coverage', async () => {
  const running = await createMockControlPlane().listen(0);
  try {
    const response = await fetch(`${running.baseUrl}/api/usage/report?profile=sample&days=30`);
    assert.equal(response.status, 200);
    const report = await response.json() as RecordedUsageReport;
    assert.equal(report.profile, 'sample');
    assert.equal(report.rows.reduce((sum: number, row: { chat_turns: number }) => sum + row.chat_turns, 0), 4);
    assert.equal(report.rows.reduce((sum: number, row: { dispatch_attempts: number }) => sum + row.dispatch_attempts, 0), 1);
    assert.equal(report.totals.total_tokens.reported_total, 3400);
    assert.equal(report.totals.actual_cost_usd.reported_total, 0.12);
    assert.equal(report.totals.actual_cost_usd.unmeasured_observations, 4);
    assert.equal(Date.parse(report.generated_at) - Date.parse(report.since), 30 * 86_400_000);
    assert.equal((await fetch(`${running.baseUrl}/api/usage/report?days=0`)).status, 400);
  } finally { await running.close(); }
});

test('sample worker factory observations are source-scoped and do not run the central controller', async () => {
  const running = await createMockControlPlane().listen(0);
  try {
    const before = await (await fetch(`${running.baseUrl}/api/loop/status`)).json();
    const result = await (await fetch(`${running.baseUrl}/api/registry/nodes/sample-worker/factory?profile=other-project`)).json() as NodeFactorySnapshot;
    assert.equal(result.node_id, 'sample-worker'); assert.equal(result.profile, 'other-project');
    assert.deepEqual(result.loop, { state: 'available', value: { running: false } });
    assert.equal(result.status.state, 'available');
    assert.ok(JSON.stringify(result).includes('Sample review gate'));
    assert.deepEqual(await (await fetch(`${running.baseUrl}/api/loop/status`)).json(), before);
  } finally { await running.close(); }
});

test('sample receipt cleanup is reviewed, replay-safe and independent of saved output', async () => {
  const running = await createMockControlPlane().listen(0);
  try {
    const base = `${running.baseUrl}/api/owner/compute-journal`;
    const review = await (await fetch(`${base}/review`)).json() as ComputeJournalReview;
    assert.equal(review.candidates.length, 1);
    const cleanup = (key: string, confirm: boolean) => fetch(`${base}/cleanup`, { method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Idempotency-Key': key }, body: JSON.stringify({ revision: review.revision, confirm }) });
    assert.equal((await cleanup('sample-receipt-rejected', false)).status, 400);
    assert.equal((await cleanup('sample-receipt-reviewed', true)).status, 200);
    assert.equal((await cleanup('sample-receipt-reviewed', true)).status, 409);
    const catalog = await (await fetch(`${running.baseUrl}/api/owner/compute-output`)).json() as ComputeOutputCatalog;
    assert.equal(catalog.journal?.entries, 3); assert.equal(catalog.entries.length, 1, 'receipt cleanup cannot discard saved output');
    await post(running.baseUrl, '/api/mock/reset');
    assert.equal((await cleanup('sample-receipt-stale-key', true)).status, 409);
  } finally { await running.close(); }
});

test('sample output review removes only simulated bytes and reset invalidates a stale selection', async () => {
  const running = await createMockControlPlane().listen(0);
  try {
    const base = `${running.baseUrl}/api/owner/compute-output`;
    const catalog = await (await fetch(base)).json() as ComputeOutputCatalog, entry = catalog.entries[0]!;
    assert.equal(entry.incomplete, true); assert.equal(catalog.admissionBlocked, false);
    const query = `revision=${entry.revision}`;
    const plan = await (await fetch(`${base}/${entry.id}/download?${query}`)).json() as ComputeOutputDownload;
    const bytes = await (await fetch(`${base}/${entry.id}/block?${query}&index=0`)).text();
    assert.match(bytes, /Simulated partial job output/); assert.equal(plan.bytes, Buffer.byteLength(bytes));
    const body = { revision: entry.revision, sha256: plan.sha256, confirm: true };
    const remove = (key: string, value = body) => fetch(`${base}/${entry.id}/discard`, { method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Idempotency-Key': key }, body: JSON.stringify(value) });
    assert.equal((await remove('sample-invalid-key', { ...body, confirm: false })).status, 400);
    assert.equal((await remove('sample-reviewed-key')).status, 200);
    assert.equal((await remove('sample-reviewed-key')).status, 409);
    assert.deepEqual((await (await fetch(base)).json() as ComputeOutputCatalog).entries, []);
    await post(running.baseUrl, '/api/mock/reset');
    assert.equal((await remove('sample-new-action-key')).status, 409);
    assert.equal((await (await fetch(base)).json() as ComputeOutputCatalog).entries.length, 1);
  } finally { await running.close(); }
});

test('sample factory dispatch acknowledges and retains exact account identity across a retry', async () => {
  const running = await createMockControlPlane().listen(0);
  const ws = new WebSocket(running.wsUrl);
  try {
    const welcome = nextMessage(ws, 'server.welcome');
    await new Promise<void>((resolve, reject) => { ws.once('open', resolve); ws.once('error', reject); });
    await welcome;
    const status = await (await fetch(`${running.baseUrl}/api/status`)).json() as { dispatch_backend_instances: boolean; backend_instances: { backend_instance: string }[] };
    assert.equal(status.dispatch_backend_instances, true);
    assert.ok(status.backend_instances.some(item => item.backend_instance === 'codex-work'));
    const message: ClientMessage = { type: 'session.start', requestId: 'sample-exact', profile: 'fixture',
      repo: 'Kh1ng/git-agent-harness', providerKind: 'codex', instanceId: 'codex-0', mode: 'improve',
      backend: 'codex', backendInstance: 'codex-work', model: 'gpt-5.3-codex', nodeId: 'mock-central' };
    let sessionId: string | undefined;
    for (let attempt = 0; attempt < 2; attempt++) {
      const reply = nextMessage(ws, 'session.started');
      ws.send(JSON.stringify(message));
      const result = await reply;
      assert.equal(result.type, 'session.started');
      if (result.type !== 'session.started') throw new Error('Expected session receipt');
      assert.equal(result.requestId, message.requestId);
      assert.equal(result.session.backendInstance, 'codex-work');
      assert.equal(result.session.nodeId, 'mock-central');
      if (sessionId) assert.equal(result.session.id, sessionId);
      sessionId = result.session.id;
    }
    const failure = nextMessage(ws, 'error');
    ws.send(JSON.stringify({ ...message, backendInstance: 'codex-personal' }));
    const error = await failure;
    assert.equal(error.type, 'error');
    const stopped = nextMessage(ws, 'session.stopped');
    ws.send(JSON.stringify({ type: 'session.stop', requestId: 'stop', sessionId }));
    const result = await stopped;
    assert.equal(result.type === 'session.stopped' && result.session.status, 'stopped');
  } finally { ws.terminate(); await running.close(); }
});

async function post(baseUrl: string, path: string, body: unknown = {}): Promise<Response> {
  return fetch(`${baseUrl}${path}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body)
  });
}

test('sample workspace checks preserve whole command lists and stay out of public profile metadata', async () => {
  const running = await createMockControlPlane().listen(0);
  const commands = ['echo PRIVATE_CHECKS_CANARY', 'node -e "console.log([1,2])"'];
  try {
    assert.equal((await fetch(`${running.baseUrl}/api/profiles/fixture`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ validation_commands: commands }) })).status, 200);
    const checks = await (await fetch(`${running.baseUrl}/api/profiles/fixture/checks`)).json() as { validation_commands: string[] };
    assert.deepEqual(checks.validation_commands, commands);
    assert.doesNotMatch(await (await fetch(`${running.baseUrl}/api/profiles`)).text(), /PRIVATE_CHECKS_CANARY/);
    await post(running.baseUrl, '/api/mock/reset');
    assert.deepEqual((await (await fetch(`${running.baseUrl}/api/profiles/fixture/checks`)).json() as { validation_commands: string[] }).validation_commands, []);
  } finally { await running.close(); }
});

test('sample environment sources round-trip presence without storing source values and reset cleanly', async () => {
  const running = await createMockControlPlane().listen(0);
  const patch = (body: object) => fetch(`${running.baseUrl}/api/profiles/fixture`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  const read = async () => (await fetch(`${running.baseUrl}/api/config/effective?profile=fixture`)).json() as Promise<{ notifications: { env_file_configured: boolean; env_file_prod_configured: boolean } }>;
  try {
    assert.equal((await patch({ env_file: 'PRIVATE_ENV_SOURCE_CANARY', env_file_prod: '/private/prod.env' })).status, 200);
    const configured = await read();
    assert.equal(configured.notifications.env_file_configured, true);
    assert.equal(configured.notifications.env_file_prod_configured, true);
    assert.doesNotMatch(JSON.stringify(configured), /PRIVATE_ENV_SOURCE_CANARY|\/private\/prod.env/);
    await patch({ clear: ['env_file_prod'] });
    assert.equal((await read()).notifications.env_file_configured, true);
    assert.equal((await read()).notifications.env_file_prod_configured, false);
    await post(running.baseUrl, '/api/mock/reset');
    assert.equal((await read()).notifications.env_file_configured, false);
  } finally { await running.close(); }
});

test('sample named account sign-in returns repair-shaped data and never asks for real credentials', async () => {
  const running = await createMockControlPlane().listen(0, '127.0.0.1');
  try {
    const response = await post(running.baseUrl, '/api/backend-instances/authenticate', { profile: 'fixture', instance: 'codex-work' });
    assert.equal(response.status, 201);
    const payload = await response.json() as { key: string; repair: { status: string; instructions: string } };
    assert.equal(payload.key.length, 64);
    assert.equal(payload.repair.status, 'manual');
    assert.match(payload.repair.instructions, /sample data/);
    assert.match(payload.repair.instructions, /No provider sign-in was started/);
  } finally { await running.close(); }
});

function nextMessage(ws: WebSocket, type: ServerMessage['type'], timeoutMs = 3_000): Promise<ServerMessage> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`timed out waiting for ${type}`)), timeoutMs);
    const listener = (raw: WebSocket.RawData) => {
      const message = JSON.parse(raw.toString()) as ServerMessage;
      if (message.type !== type) return;
      clearTimeout(timer);
      ws.off('message', listener);
      resolve(message);
    };
    ws.on('message', listener);
  });
}

function frontendApiRoutes(): { method: string; path: string }[] {
  const clientPath = resolve(dirname(fileURLToPath(import.meta.url)), '../../web/src/api/client.ts');
  const source = ts.createSourceFile(clientPath, readFileSync(clientPath, 'utf8'), ts.ScriptTarget.Latest, true);
  const helperMethods = new Map([
    ['getJson', 'GET'],
    ['postJson', 'POST'],
    ['patchJson', 'PATCH'],
    ['putJson', 'PUT'],
    ['deleteJson', 'DELETE']
  ]);
  const routes: { method: string; path: string }[] = [];

  function routePath(node: ts.Expression | undefined): string | null {
    if (!node) return null;
    if (ts.isStringLiteralLike(node)) return node.text.startsWith('/api/') ? node.text : null;
    if (!ts.isTemplateExpression(node) || !node.head.text.startsWith('/api/')) return null;
    return node.head.text + node.templateSpans.map((span) => `:param${span.literal.text}`).join('');
  }

  function visit(node: ts.Node): void {
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)) {
      const method = helperMethods.get(node.expression.text);
      const path = routePath(node.arguments[0]);
      if (method && path) routes.push({ method, path: path.split('?')[0] });
    } else if (ts.isNewExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'URL') {
      const path = routePath(node.arguments?.[0]);
      if (path) routes.push({ method: 'POST', path: path.split('?')[0] });
    }
    ts.forEachChild(node, visit);
  }
  visit(source);

  const deadClientMethods = new Set([
    'POST /api/projects',
    'DELETE /api/projects/:param',
    // Same dead deleteProject call site: its `?nodeId=` query-string
    // ternary parses as a second template span, so the route shows up as
    // /api/projects/:param:param -- the query string, not a real path
    // segment.
    'DELETE /api/projects/:param:param',
    'POST /api/context/recall',
    'GET /api/git/branches'
  ]);
  return routes.filter(({ method, path }) => !deadClientMethods.has(`${method} ${path}`));
}

test('named scenarios are discoverable, switchable, and resettable in memory', async () => {
  const running = await createMockControlPlane().listen(0);
  try {
    const catalog = await fetch(`${running.baseUrl}/api/mock/scenarios`).then((response) => response.json()) as {
      active: string;
      scenarios: { name: string }[];
    };
    assert.equal(catalog.active, 'normal');
    assert.deepEqual(catalog.scenarios.map(({ name }) => name), Object.keys(MOCK_SCENARIOS));

    const selected = await post(running.baseUrl, '/api/mock/scenario', { name: 'archive-success' });
    assert.equal(selected.status, 200);
    assert.equal(running.scenario(), 'archive-success');

    const archived = await post(running.baseUrl, '/api/manager-chat/sessions/archive', {
      profile: 'fixture',
      sessionId: 'mock-session-1'
    });
    assert.equal(archived.status, 200);
    assert.equal((await archived.json() as { archivedAt: number | null }).archivedAt === null, false);

    const restored = await post(running.baseUrl, '/api/manager-chat/sessions/restore', {
      profile: 'fixture', sessionId: 'mock-session-1'
    });
    assert.equal(restored.status, 200);
    assert.equal((await restored.json() as { archivedAt: number | null }).archivedAt, null);

    await post(running.baseUrl, '/api/mock/reset');
    const sessions = await fetch(`${running.baseUrl}/api/manager-chat/sessions?profile=fixture`).then((response) => response.json()) as {
      sessions: { archivedAt: number | null }[];
    };
    assert.equal(sessions.sessions[0]?.archivedAt, null);

    const storage = await fetch(`${running.baseUrl}/api/manager-chat/storage?profile=fixture`).then((response) => response.json()) as {
      dryRun: boolean;
      candidates: { sessionId: string; reclaimBytes: number }[];
    };
    assert.equal(storage.dryRun, true);
    assert.deepEqual(storage.candidates, [{ profile: 'fixture', sessionId: 'mock-session-1', outcome: 'archived', reason: 'idle', reclaimBytes: 12_582_912 }]);

    const bulk = await post(running.baseUrl, '/api/manager-chat/sessions/archive', {
      profile: 'fixture', sessionIds: ['mock-session-1']
    });
    assert.equal(bulk.status, 200);
    assert.equal((await bulk.json() as { sessions: { outcome: string }[] }).sessions[0]?.outcome, 'archived');
  } finally {
    await running.close();
  }
});

test('normal scenario streams contract-shaped chunks, tools, and completion over a real socket', async () => {
  const running = await createMockControlPlane({ scenario: 'normal' }).listen(0);
  const ws = new WebSocket(running.wsUrl);
  try {
    const welcomePromise = nextMessage(ws, 'server.welcome');
    await new Promise<void>((resolve, reject) => {
      ws.once('open', resolve);
      ws.once('error', reject);
    });
    await welcomePromise;

    ws.send(JSON.stringify({
      type: 'manager.chat.historyRequest',
      requestId: 'history-1',
      profile: 'fixture'
    } satisfies ClientMessage));
    const history = await nextMessage(ws, 'manager.chat.history');
    assert.equal(history.type === 'manager.chat.history' && history.turns.length, 0);

    const chunkPromise = nextMessage(ws, 'manager.chat.chunk');
    const toolPromise = nextMessage(ws, 'manager.chat.toolCall');
    const replyPromise = nextMessage(ws, 'manager.chat.reply');
    ws.send(JSON.stringify({
      type: 'manager.chat.send',
      requestId: 'turn-1',
      profile: 'fixture',
      message: 'exercise the typed stream'
    } satisfies ClientMessage));

    const chunk = await chunkPromise;
    const tool = await toolPromise;
    const reply = await replyPromise;
    assert.equal(chunk.type, 'manager.chat.chunk');
    assert.equal(tool.type, 'manager.chat.toolCall');
    assert.equal(reply.type === 'manager.chat.reply' && reply.reply, 'Mock turn complete after multiple chunks.');
  } finally {
    ws.close();
    await running.close();
  }
});

test('failure scenarios are explicit REST and WS failures with no state writes', async () => {
  const running = await createMockControlPlane({ scenario: 'archive-failure' }).listen(0);
  try {
    const failed = await post(running.baseUrl, '/api/manager-chat/sessions/archive', {
      profile: 'fixture', sessionId: 'mock-session-1'
    });
    assert.equal(failed.status, 502);
    const sessions = await fetch(`${running.baseUrl}/api/manager-chat/sessions?profile=fixture`).then((response) => response.json()) as {
      sessions: { archivedAt: number | null }[];
    };
    assert.equal(sessions.sessions[0]?.archivedAt, null);

    await post(running.baseUrl, '/api/mock/scenario', { name: 'ws-error' });
    const ws = new WebSocket(running.wsUrl);
    const welcomePromise = nextMessage(ws, 'server.welcome');
    await new Promise<void>((resolve, reject) => {
      ws.once('open', resolve);
      ws.once('error', reject);
    });
    await welcomePromise;
    const errorPromise = nextMessage(ws, 'error');
    ws.send(JSON.stringify({
      type: 'manager.chat.send', requestId: 'turn-error', profile: 'fixture', message: 'fail'
    } satisfies ClientMessage));
    const error = await errorPromise;
    assert.equal(error.type === 'error' && error.error, 'Mock WebSocket turn failure');
    ws.close();
  } finally {
    await running.close();
  }
});

test('PR start opens a worktree-less session whose history is seeded with the PR', async () => {
  const running = await createMockControlPlane().listen(0);
  try {
    const prs = await fetch(`${running.baseUrl}/api/manager-chat/prs?profile=fixture`).then((response) => response.json()) as {
      prs: { number: number; title: string; author: string; isDraft: boolean; reviewState: string | null }[];
    };
    assert.deepEqual(prs.prs.map((pr) => pr.number), [12, 11]);
    assert.equal(prs.prs[0].author, 'octocat');
    assert.equal(prs.prs[1].isDraft, true);

    const missing = await post(running.baseUrl, '/api/manager-chat/prs/start', { profile: 'fixture' });
    assert.equal(missing.status, 400);

    const started = await post(running.baseUrl, '/api/manager-chat/prs/start', { profile: 'fixture', prNumber: 12 });
    assert.equal(started.status, 201);
    const { session } = await started.json() as { session: { id: string; worktreePath: string | null; branch: string; title: string } };
    assert.equal(session.worktreePath, null, 'read-only: no worktree');
    assert.equal(session.branch, 'feat/pr-chat');
    assert.equal(session.title, '#12 Ship the PR chat mode');

    const ws = new WebSocket(running.wsUrl);
    const welcomePromise = nextMessage(ws, 'server.welcome');
    await new Promise<void>((resolve, reject) => {
      ws.once('open', resolve);
      ws.once('error', reject);
    });
    await welcomePromise;
    ws.send(JSON.stringify({
      type: 'manager.chat.historyRequest',
      requestId: 'pr-history',
      profile: 'fixture',
      sessionId: session.id
    } satisfies ClientMessage));
    const history = await nextMessage(ws, 'manager.chat.history');
    assert.equal(history.type === 'manager.chat.history' && history.turns[0]?.role, 'user');
    assert.equal(history.type === 'manager.chat.history' && history.turns[0]?.text.includes('#12 Ship the PR chat mode'), true);
    ws.close();
  } finally {
    await running.close();
  }
});

test('Settings mutations persist without storing secrets and reset to defaults', async () => {
  const running = await createMockControlPlane().listen(0);
  try {
    const gateway = await fetch(`${running.baseUrl}/api/settings/gateway`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ enabled: false, apiKey: 'must-not-serialize', contextPolicy: { budgetChars: 1_200, tiers: ['L0'] } })
    });
    assert.equal(gateway.status, 200);

    const profile = await fetch(`${running.baseUrl}/api/profiles/fixture`, {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ validation_timeout_seconds: 900 })
    });
    assert.equal(profile.status, 200);

    const changed = await fetch(`${running.baseUrl}/api/mock/state`).then((response) => response.json()) as {
      gateway: { enabled: boolean; apiKeyConfigured: boolean; contextPolicy: { budgetChars?: number } };
      profiles: { validation_timeout_seconds: number }[];
    };
    assert.equal(changed.gateway.enabled, false);
    assert.equal(changed.gateway.apiKeyConfigured, true);
    assert.equal(changed.gateway.contextPolicy.budgetChars, 1_200);
    assert.equal(changed.profiles[0]?.validation_timeout_seconds, 900);
    assert.equal(JSON.stringify(changed).includes('must-not-serialize'), false);

    await post(running.baseUrl, '/api/mock/reset');
    const reset = await fetch(`${running.baseUrl}/api/mock/state`).then((response) => response.json()) as typeof changed;
    assert.equal(reset.gateway.enabled, true);
    assert.equal(reset.gateway.contextPolicy.budgetChars, 4_000);
    assert.equal(reset.profiles[0]?.validation_timeout_seconds, 300);
  } finally {
    await running.close();
  }
});

test('new non-chat mutations return success and persist in memory', async () => {
  const running = await createMockControlPlane().listen(0);
  try {
    const added = await post(running.baseUrl, '/api/profiles', {
      name: 'added',
      display_name: 'Added',
      repo_id: 'added',
      provider: 'github',
      repo: 'fixture/added',
      local_path: '/mock/added',
      artifact_root: '/mock/artifacts'
    });
    assert.equal(added.status, 201);
    assert.equal((await fetch(`${running.baseUrl}/api/profiles`).then((response) => response.json()) as { name: string }[]).some(({ name }) => name === 'added'), true);

    const patched = await fetch(`${running.baseUrl}/api/profiles/added`, {
      method: 'PATCH', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ validation_timeout_seconds: 600 })
    });
    assert.equal(patched.status, 200);
    assert.equal((await fetch(`${running.baseUrl}/api/profiles/added`, { method: 'DELETE' })).status, 200);

    const imported = await post(running.baseUrl, '/api/projects/import', { gitUrl: 'https://github.com/fixture/imported.git' });
    assert.equal(imported.status, 201);
    assert.equal((await imported.json() as ProjectImportResult).project.repo, 'fixture/imported');

    assert.equal((await post(running.baseUrl, '/api/loop/stop', { profile: 'fixture' })).status, 200);
    assert.equal((await post(running.baseUrl, '/api/loop/start', { profile: 'fixture' })).status, 200);
    assert.equal((await post(running.baseUrl, '/api/config', { current_manager: 'claude' })).status, 200);

    const pullRequest = await post(running.baseUrl, '/api/git/pr', { title: 'Mock pull request' });
    assert.equal(pullRequest.status, 200);
    const prs = await fetch(`${running.baseUrl}/api/git/prs`).then((response) => response.json()) as { prs: { number: number; title: string }[] };
    assert.equal(prs.prs[0]?.title, 'Mock pull request');
    const update = await post(running.baseUrl, '/api/git/pull-request/update', { number: prs.prs[0].number, title: 'Edited mock pull request', body: 'Edited body' });
    assert.equal(update.status, 200);
    const updatedPrs = await fetch(`${running.baseUrl}/api/git/prs`).then((response) => response.json()) as { prs: { title: string }[] };
    assert.equal(updatedPrs.prs[0]?.title, 'Edited mock pull request');

    const createdSkill = await post(running.baseUrl, '/api/skills', {
      id: 'review', version: '1.0.0', displayName: 'Review', content: '# Review', backends: ['codex']
    });
    assert.equal(createdSkill.status, 201);
    const createdAt = (await createdSkill.json() as { createdAt: number }).createdAt;
    const updatedSkill = await post(running.baseUrl, '/api/skills', {
      id: 'review', version: '1.0.0', displayName: 'Review edits', content: '# Review edits', backends: ['codex']
    });
    assert.equal(updatedSkill.status, 200);
    assert.equal((await updatedSkill.json() as { createdAt: number }).createdAt, createdAt);
    assert.equal((await fetch(`${running.baseUrl}/api/skills/review?version=1.0.0`).then(response => response.json()) as { content: string }).content, '# Review edits');
    assert.equal((await fetch(`${running.baseUrl}/api/skills/review`, { method: 'DELETE' })).status, 200);

    assert.equal((await post(running.baseUrl, '/api/manager-chat/issues/start', { profile: 'fixture', issueNumber: 1087 })).status, 201);
    assert.equal((await post(running.baseUrl, '/api/admin/update')).status, 202);
  } finally {
    await running.close();
  }
});

test('rest-error rejects every used frontend mutation without changing state', async () => {
  const running = await createMockControlPlane({ scenario: 'rest-error' }).listen(0);
  try {
    for (const { method, path } of frontendApiRoutes().filter(({ method }) => method !== 'GET')) {
      const failed = await fetch(`${running.baseUrl}${path.replaceAll(':param', 'fixture')}`, {
        method,
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ enabled: false })
      });
      assert.equal(failed.status, 503, `${method} ${path}`);
    }
    const state = await fetch(`${running.baseUrl}/api/mock/state`).then((response) => response.json()) as {
      gateway: { enabled: boolean };
    };
    assert.equal(state.gateway.enabled, true);
  } finally {
    await running.close();
  }
});

test('every used frontend API call has a registered mock route', async () => {
  const running = await createMockControlPlane().listen(0);
  try {
    for (const { method, path } of frontendApiRoutes()) {
      // Each route needs the seeded fixture: an earlier DELETE otherwise
      // makes a later resource read look like an unregistered endpoint.
      await post(running.baseUrl, '/api/mock/reset');
      const requestPath = path.replaceAll(':param', 'fixture');
      const response = await fetch(`${running.baseUrl}${requestPath}`, {
        method,
        ...(method === 'GET' ? {} : {
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({})
        })
      });
      assert.notEqual(response.status, 404, `${method} ${path}`);
    }
  } finally {
    await running.close();
  }
});

test('mock approval requests and device delegation round-trip through fixtures and reset', async () => {
  const running = await createMockControlPlane().listen(0);
  try {
    const session = await (await fetch(`${running.baseUrl}/api/pairing/session`)).json() as { can_approve_pairing: boolean };
    assert.equal(session.can_approve_pairing, true);
    const request = await (await post(running.baseUrl, '/api/pairing/access/request', { name: 'Linux browser' })).json() as PairingAccessRequest;
    assert.equal(request.status, 'pending');
    assert.equal(request.name, 'Linux browser');
    assert.equal((await post(running.baseUrl, '/api/pairing/access/claim', { confirm: true })).status, 400);
    const approved = await post(running.baseUrl, `/api/pairing/access/requests/${request.id}/approve`, { matching_code: request.matching_code, confirm: true });
    assert.equal((await approved.json() as PairingAccessRequest).status, 'approved');
    const status = await (await fetch(`${running.baseUrl}/api/pairing/access/status`)).json() as PairingAccessRequest;
    assert.equal(status.id, request.id);
    assert.equal(status.status, 'approved');
    const claim = await (await post(running.baseUrl, '/api/pairing/access/claim', { confirm: true })).json() as { device: PairedDevice };
    assert.equal(claim.device.name, request.name);
    assert.equal(claim.device.can_approve_pairing, false);
    assert.equal((await post(running.baseUrl, '/api/pairing/access/claim', { confirm: true })).status, 400);
    const delegated = await fetch(`${running.baseUrl}/api/pairing/devices/mock-device-1`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ can_approve_pairing: true }) });
    assert.equal((await delegated.json() as { device: PairedDevice }).device.can_approve_pairing, true);
    const denied = await (await post(running.baseUrl, '/api/pairing/access/request', { name: 'Denied browser' })).json() as PairingAccessRequest;
    assert.equal((await (await post(running.baseUrl, `/api/pairing/access/requests/${denied.id}/deny`)).json() as PairingAccessRequest).status, 'denied');
    await post(running.baseUrl, '/api/mock/reset');
    const devices = await (await fetch(`${running.baseUrl}/api/pairing/devices`)).json() as { devices: PairedDevice[] };
    assert.equal(devices.devices.length, 1);
    assert.equal(devices.devices[0].can_approve_pairing, false);
  } finally { await running.close(); }
});
