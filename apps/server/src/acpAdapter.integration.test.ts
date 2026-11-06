import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { test } from 'node:test';
import type { ChatTranscriptTurn } from '@git-agent-harness/contracts';

import { createAcpBackend } from './managerChat/acpAdapter.js';
import { defaultClaudeSpawn, resolveAdapter, invalidateAgentModels } from './managerChat/registry.js';

const FAKE_ACP_SOURCE = String.raw`
const fs = require('node:fs');
const readline = require('node:readline');

const mode = process.argv[2];
const spawnCountPath = process.argv[3];
let spawnNumber = 1;
if (spawnCountPath) {
  const previous = fs.existsSync(spawnCountPath) ? Number(fs.readFileSync(spawnCountPath, 'utf8')) : 0;
  spawnNumber = previous + 1;
  fs.writeFileSync(spawnCountPath, String(spawnNumber));
}
let promptNumber = 0;
if (mode === 'archive-tree') {
  const child = require('node:child_process').spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
  fs.appendFileSync(spawnCountPath + '.pids', JSON.stringify([process.pid, child.pid]) + '\n');
}
let pendingPrompt;
let usageTurns = 0;
const lines = readline.createInterface({ input: process.stdin });

function respond(message) {
  process.stdout.write(JSON.stringify(message) + '\n');
}

lines.on('line', (line) => {
  const request = JSON.parse(line);
  if (request.method === 'session/prompt' && (mode === 'usage-breakdown' || mode === 'usage-gap')) {
    usageTurns++;
    if (mode === 'usage-gap' && usageTurns === 2) {
      respond({ jsonrpc: '2.0', id: request.id, result: { stopReason: 'end_turn' } });
      return;
    }
    respond({ jsonrpc: '2.0', id: request.id, result: { stopReason: 'end_turn', usage: {
      inputTokens: usageTurns * 10, outputTokens: usageTurns * 5, totalTokens: usageTurns * 100,
      cachedReadTokens: usageTurns * 80, cachedWriteTokens: 0, thoughtTokens: usageTurns * 3
    } } });
    return;
  }
  if (request.method === 'initialize') {
    respond({ jsonrpc: '2.0', id: request.id, result: { protocolVersion: request.params.protocolVersion, _meta: { steering: { supported: true } } } });
    return;
  }
  if (request.method === 'session/new' && mode === 'runtime-models') {
    // Advertise a model list that names the runtime this child was spawned for.
    const runtime = process.env.CLAUDE_CODE_EXECUTABLE ? 'installed' : 'bundled';
    respond({ jsonrpc: '2.0', id: request.id, result: { sessionId: 'fake-session', configOptions: [{
      id: 'model', name: 'Model', type: 'select', category: 'model', currentValue: runtime,
      options: [{ value: runtime, name: runtime }] }] } });
    return;
  }
  if (request.method === 'session/new' && mode === 'workspace-env') {
    const model = process.env.APP_LABEL + '|' + process.env.APP_OVERRIDE;
    respond({ jsonrpc: '2.0', id: request.id, result: { sessionId: 'fake-session', configOptions: [{
      id: 'model', name: 'Model', type: 'select', category: 'model', currentValue: model,
      options: [{ value: model, name: model }] }] } });
    return;
  }
  if (request.method === 'session/new') {
    respond({ jsonrpc: '2.0', id: request.id, result: { sessionId: 'fake-session' } });
    return;
  }
  if (mode === 'refresh-cancel') {
    if (request.method === 'session/prompt') {
      pendingPrompt = request.id;
      respond({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: 'fake-session',
        update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'active' } } } });
      return;
    }
    if (request.method === '_session/steering') {
      fs.writeFileSync(spawnCountPath + '.steered', 'yes');
      respond({ jsonrpc: '2.0', id: request.id, result: { outcome: 'injected' } });
      return;
    }
    if (request.method === 'session/cancel') {
      fs.writeFileSync(spawnCountPath + '.cancelled', 'yes');
      respond({ jsonrpc: '2.0', id: pendingPrompt, result: { stopReason: 'cancelled' } });
      return;
    }
  }
  if (request.method === 'session/prompt' && mode === 'retire-prompt') {
    fs.writeFileSync(spawnCountPath + '.pid', String(process.pid));
    respond({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: 'fake-session',
      update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'active-turn' } } } });
    setTimeout(() => respond({ jsonrpc: '2.0', id: request.id, result: { stopReason: 'end_turn' } }), 100);
    return;
  }
  if (request.method === 'session/prompt' && mode === 'detail-less-error') {
    process.stderr.write('SECRET-START\n' + 'x'.repeat(5000) + '\nDIAGNOSTIC-END\n', () => {
      respond({ jsonrpc: '2.0', id: request.id, error: { code: -32603, message: 'Internal error' } });
      setTimeout(() => process.exit(17), 25);
    });
    return;
  }
  if (request.method === 'session/prompt' && mode === 'detailed-error') {
    respond({
      jsonrpc: '2.0',
      id: request.id,
      error: { code: -32603, message: 'Internal error', data: { message: 'Useful nested detail' } }
    });
    setTimeout(() => process.exit(0), 25);
    return;
  }
  if (request.method === 'session/prompt' && mode === 'env-reply') {
    const text = ['COORDINATOR_TOKEN', 'TDAI_GATEWAY_API_KEY', 'PATH'].map((name) => name + '=' + (name in process.env ? 'set' : 'unset')).join(' ');
    respond({ jsonrpc: '2.0', method: 'session/update', params: { sessionId: 'fake-session',
      update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text } } } });
    respond({ jsonrpc: '2.0', id: request.id, result: { stopReason: 'end_turn' } });
    return;
  }
  if (request.method === 'session/prompt' && mode === 'reconnect-sequence') {
    promptNumber += 1;
    if (spawnNumber === 1 && promptNumber !== 2) {
      respond({ jsonrpc: '2.0', id: request.id, error: { code: -32603, message: 'Internal error' } });
      return;
    }
    const text = spawnNumber === 1
      ? 'recovered-on-original-child'
      : request.params.prompt[0].text.includes('user: before') ? 'rehydrated-on-new-child' : 'history-missing';
    respond({
      jsonrpc: '2.0',
      method: 'session/update',
      params: {
        sessionId: 'fake-session',
        update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text } }
      }
    });
    respond({ jsonrpc: '2.0', id: request.id, result: { stopReason: 'end_turn' } });
    if (spawnNumber > 1) setTimeout(() => process.exit(0), 25);
    return;
  }
  if (request.method === 'session/prompt' && mode === 'stuck-cancel') {
    if (spawnNumber === 1) {
      respond({
        jsonrpc: '2.0',
        method: 'session/update',
        params: {
          sessionId: 'fake-session',
          update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'stuck-prompt-started' } }
        }
      });
      return;
    }
    respond({
      jsonrpc: '2.0',
      method: 'session/update',
      params: {
        sessionId: 'fake-session',
        update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'recovered-after-cancel' } }
      }
    });
    respond({ jsonrpc: '2.0', id: request.id, result: { stopReason: 'end_turn' } });
    setTimeout(() => process.exit(0), 25);
  }
});
`;

function fakeAcpScript(): { dir: string; path: string } {
  const dir = mkdtempSync(join(tmpdir(), 'gah-fake-acp-'));
  const scriptPath = join(dir, 'fake-acp.cjs');
  writeFileSync(scriptPath, FAKE_ACP_SOURCE);
  return { dir, path: scriptPath };
}

test('ACP replies preserve cache and reasoning categories across real protocol turns', async () => {
  const fake = fakeAcpScript();
  const backend = createAcpBackend('Usage fixture', () => ({ command: process.execPath, args: [fake.path, 'usage-breakdown'] }));
  try {
    const history: ChatTranscriptTurn[] = [];
    for (let turn = 0; turn < 2; turn++) {
      const result = await backend.runTurn({ key: 'usage-fixture', profile: 'fixture' }, { ...emptyTurnInput, prompt: 'usage fixture', history: [...history] });
      assert.deepEqual(result.usage, {
        input_tokens: 10, output_tokens: 5, total_tokens: 100,
        cache_read_tokens: 80, cache_write_tokens: 0, reasoning_tokens: 3,
        estimated_cost_usd: null, duration_seconds: result.usage?.duration_seconds
      });
      history.push({ role: 'user', text: 'usage fixture', timestamp: 1 }, { role: 'assistant', text: result.reply, timestamp: 2 });
    }
  } finally { backend.dispose(); rmSync(fake.dir, { recursive: true, force: true }); }
});

test('a missing ACP usage report cannot charge several turns to the next reply', async () => {
  const fake = fakeAcpScript();
  const backend = createAcpBackend('Usage gap', () => ({ command: process.execPath, args: [fake.path, 'usage-gap'] }));
  try {
    const history: ChatTranscriptTurn[] = [];
    for (const total of [100, null, null, 100]) {
      const result = await backend.runTurn({ key: 'usage-gap', profile: 'fixture' }, { ...emptyTurnInput, history: [...history] });
      assert.equal(result.usage?.total_tokens ?? null, total);
      assert.equal(result.usage?.cache_read_tokens ?? null, total === null ? null : 80);
      history.push({ role: 'user', text: emptyTurnInput.prompt, timestamp: 1 }, { role: 'assistant', text: result.reply, timestamp: 2 });
    }
  } finally { backend.dispose(); rmSync(fake.dir, { recursive: true, force: true }); }
});

test('ACP workspace startup uses the real profile and shares concurrent connection reads', async () => {
  const fake = fakeAcpScript();
  const count = join(fake.dir, 'count');
  let reads = 0;
  const backend = createAcpBackend('Workspace', () => ({ command: process.execPath, args: [fake.path, 'workspace-env', count], env: { APP_OVERRIDE: 'adapter' } }), {
    environment: async ({ profile }) => { reads++; await Promise.resolve(); return { APP_LABEL: profile, APP_OVERRIDE: 'workspace' }; }
  });
  try {
    const conversation = { key: 'opaque/not-a-profile', profile: 'real-project' };
    const [first, second] = await Promise.all([backend.listModels(conversation), backend.listModels(conversation)]);
    assert.equal(first.currentModelId, 'real-project|adapter');
    assert.equal(second.currentModelId, first.currentModelId);
    assert.equal(reads, 1);
    assert.equal(readFileSync(count, 'utf8'), '1');
  } finally { backend.dispose(); rmSync(fake.dir, { recursive: true, force: true }); }
});

test('a failed workspace read can recover on the next ACP request', async () => {
  const fake = fakeAcpScript();
  let reads = 0;
  const backend = createAcpBackend('Workspace', () => ({ command: process.execPath, args: [fake.path, 'workspace-env'] }), {
    environment: async () => {
      if (++reads === 1) throw new Error('Workspace source unavailable');
      return { APP_LABEL: 'recovered', APP_OVERRIDE: 'workspace' };
    }
  });
  try {
    const conversation = { key: 'recoverable-conversation', profile: 'project' };
    await assert.rejects(backend.listModels(conversation), /Workspace source unavailable/);
    assert.equal((await backend.listModels(conversation)).currentModelId, 'recovered|workspace');
    assert.equal(reads, 2);
  } finally { backend.dispose(); rmSync(fake.dir, { recursive: true, force: true }); }
});

test('cancel, release and disposal during workspace reads cannot launch a late ACP child', async () => {
  for (const action of ['cancel', 'release', 'dispose']) {
    let release!: (env: Record<string, string>) => void;
    let spawned = false;
    const backend = createAcpBackend('Workspace', () => { spawned = true; throw new Error('must not spawn'); }, {
      environment: () => new Promise(resolve => { release = resolve; })
    });
    const pending = backend.listModels({ key: 'conversation', profile: 'project' });
    if (action === 'cancel') await backend.cancelTurn('conversation');
    else if (action === 'release') await backend.releaseConversation('conversation');
    else backend.dispose();
    release({ APP_LABEL: 'late' });
    await assert.rejects(pending, /startup cancelled/);
    assert.equal(spawned, false);
    backend.dispose();
  }
});

test('releasing an archived ACP conversation stops its process tree and preserves siblings', { timeout: 30_000 }, async () => {
  const fake = fakeAcpScript();
  const count = join(fake.dir, 'count');
  const backend = createAcpBackend('Archive', () => ({ command: process.execPath, args: [fake.path, 'archive-tree', count] }));
  const first = { key: 'first-session', profile: 'project' };
  const second = { key: 'second-session', profile: 'project' };
  const alive = (pid: number) => { try { process.kill(pid, 0); return true; } catch { return false; } };
  try {
    await backend.listModels(first);
    await backend.listModels(second);
    const [firstPids, secondPids] = readFileSync(count + '.pids', 'utf8').trim().split('\n').map(line => JSON.parse(line) as number[]);
    assert.ok([...firstPids, ...secondPids].every(alive));
    await backend.releaseConversation(first.key);
    assert.ok(firstPids.every(pid => !alive(pid)), 'bridge and descendant both exit before release returns');
    assert.ok(secondPids.every(alive), 'sibling conversation remains available');
    await backend.listModels(second);
    assert.equal(readFileSync(count, 'utf8'), '2');
    await backend.listModels(first);
    assert.equal(readFileSync(count, 'utf8'), '3', 'restoring the conversation starts a new runtime');
  } finally {
    await Promise.all([backend.releaseConversation(first.key), backend.releaseConversation(second.key)]);
    backend.dispose();
    rmSync(fake.dir, { recursive: true, force: true });
  }
});

const emptyTurnInput = {
  prompt: 'hello',
  history: [],
  onChunk: () => undefined,
  onToolResult: () => undefined
};

test('a missing ACP command rejects the turn instead of crashing the server', async () => {
  const backend = createAcpBackend('Hermes', () => ({
    command: join(tmpdir(), 'gah-acp-command-that-does-not-exist'),
    args: ['acp']
  }));

  await assert.rejects(
    backend.runTurn({ key: 'profile', profile: 'profile' }, emptyTurnInput),
    /Hermes is not installed on this machine: the ".*gah-acp-command-that-does-not-exist" command was not found\./
  );
  // The failed connection must not be cached: the next turn tries again.
  await assert.rejects(backend.runTurn({ key: 'profile', profile: 'profile' }, emptyTurnInput), /Hermes is not installed/);
});

test('detail-less ACP rejection surfaces code and bounded child diagnostics', async () => {
  const fake = fakeAcpScript();
  try {
    const backend = createAcpBackend('Codex', () => ({
      command: process.execPath,
      args: [fake.path, 'detail-less-error']
    }));

    await assert.rejects(
      backend.runTurn({ key: 'profile', profile: 'profile' }, emptyTurnInput),
      (error: unknown) => {
        assert.ok(error instanceof Error);
        assert.match(error.message, /^Internal error \[ACP code=-32603;/);
        assert.match(error.message, /stderr tail=.*DIAGNOSTIC-END/);
        assert.doesNotMatch(error.message, /SECRET-START/);
        assert.ok(error.message.length < 4_500, `diagnostic error was ${error.message.length} characters`);
        return true;
      }
    );
  } finally {
    await new Promise((resolve) => setTimeout(resolve, 50));
    rmSync(fake.dir, { recursive: true, force: true });
  }
});

test('ACP rejection preserves nested detail without diagnostic decoration', async () => {
  const fake = fakeAcpScript();
  try {
    const backend = createAcpBackend('Codex', () => ({
      command: process.execPath,
      args: [fake.path, 'detailed-error']
    }), { consecutiveFailureReconnectThreshold: 2 });

    await assert.rejects(
      backend.runTurn({ key: 'profile', profile: 'profile' }, emptyTurnInput),
      (error: unknown) => error instanceof Error && error.message === 'Useful nested detail'
    );
  } finally {
    await new Promise((resolve) => setTimeout(resolve, 50));
    rmSync(fake.dir, { recursive: true, force: true });
  }
});

test('Codex evicts after two consecutive ACP failures and rehydrates on the next turn', async () => {
  const fake = fakeAcpScript();
  const spawnCountPath = join(fake.dir, 'spawn-count');
  const durableHistory = [
    { role: 'user' as const, text: 'before', timestamp: 1 },
    { role: 'assistant' as const, text: 'durable answer', timestamp: 2 }
  ];
  try {
    const backend = createAcpBackend('Codex', () => ({
      command: process.execPath,
      args: [fake.path, 'reconnect-sequence', spawnCountPath]
    }), { consecutiveFailureReconnectThreshold: 2 });

    await assert.rejects(
      backend.runTurn({ key: 'profile', profile: 'profile' }, { ...emptyTurnInput, prompt: 'fails once', history: durableHistory }),
      /consecutive failures=1\/2/
    );
    const recovered = await backend.runTurn({ key: 'profile', profile: 'profile' }, {
      ...emptyTurnInput,
      prompt: 'succeeds and resets the counter',
      history: durableHistory
    });
    assert.equal(recovered.reply, 'recovered-on-original-child');

    const currentHistory = [
      ...durableHistory,
      { role: 'user' as const, text: 'succeeds and resets the counter', timestamp: 3 },
      { role: 'assistant' as const, text: recovered.reply, timestamp: 4 }
    ];
    await assert.rejects(
      backend.runTurn({ key: 'profile', profile: 'profile' }, { ...emptyTurnInput, prompt: 'fails after success', history: currentHistory }),
      /consecutive failures=1\/2/
    );
    await assert.rejects(
      backend.runTurn({ key: 'profile', profile: 'profile' }, { ...emptyTurnInput, prompt: 'second consecutive failure', history: currentHistory }),
      /consecutive failures=2\/2; connection evicted/
    );

    const rehydrated = await backend.runTurn({ key: 'profile', profile: 'profile' }, {
      ...emptyTurnInput,
      prompt: 'next turn',
      history: currentHistory
    });
    assert.equal(rehydrated.reply, 'rehydrated-on-new-child');
    assert.equal(Number(readFileSync(spawnCountPath, 'utf8')), 2);
  } finally {
    await new Promise((resolve) => setTimeout(resolve, 50));
    rmSync(fake.dir, { recursive: true, force: true });
  }
});

test('cancel evicts a child only when its prompt ignores cancellation', { timeout: 5_000 }, async () => {
  const fake = fakeAcpScript();
  const spawnCountPath = join(fake.dir, 'spawn-count');
  try {
    const backend = createAcpBackend('Claude', () => ({
      command: process.execPath,
      args: [fake.path, 'stuck-cancel', spawnCountPath]
    }));

    let markPromptStarted!: () => void;
    const promptStarted = new Promise<void>((resolve) => { markPromptStarted = resolve; });
    const stuck = backend.runTurn({ key: 'profile', profile: 'profile' }, {
      ...emptyTurnInput,
      onChunk: (text) => { if (text === 'stuck-prompt-started') markPromptStarted(); }
    });
    await promptStarted;
    await assert.rejects(backend.releaseConversation('profile'), /Stop this turn/, 'archive cleanup must not interrupt a live prompt');
    await backend.cancelTurn('profile');
    await assert.rejects(stuck);

    const recovered = await backend.runTurn({ key: 'profile', profile: 'profile' }, emptyTurnInput);
    assert.equal(recovered.reply, 'recovered-after-cancel');
    assert.equal(Number(readFileSync(spawnCountPath, 'utf8')), 2);
  } finally {
    await new Promise((resolve) => setTimeout(resolve, 50));
    rmSync(fake.dir, { recursive: true, force: true });
  }
});


test('an ACP child without spec env still gets only the allow-listed environment', async () => {
  const fake = fakeAcpScript();
  const saved = { COORDINATOR_TOKEN: process.env.COORDINATOR_TOKEN, TDAI_GATEWAY_API_KEY: process.env.TDAI_GATEWAY_API_KEY };
  process.env.COORDINATOR_TOKEN = 'owner-token';
  process.env.TDAI_GATEWAY_API_KEY = 'gateway-key';
  const backend = createAcpBackend('Hermes', () => ({ command: process.execPath, args: [fake.path, 'env-reply'] }));
  try {
    const result = await backend.runTurn({ key: 'profile', profile: 'profile' }, emptyTurnInput);
    assert.equal(result.reply, 'COORDINATOR_TOKEN=unset TDAI_GATEWAY_API_KEY=unset PATH=set');
  } finally {
    backend.dispose();
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(fake.dir, { recursive: true, force: true });
  }
});

test('credential rotation retires an ACP child after its current prompt settles', { timeout: 3000 }, async () => {
  const fake = fakeAcpScript();
  const count = join(fake.dir, 'count');
  const backend = createAcpBackend('Codex', () => ({ command: process.execPath, args: [fake.path, 'retire-prompt', count] }));
  try {
    let started!: () => void;
    const ready = new Promise<void>(resolve => { started = resolve; });
    const turn = backend.runTurn({ key: 'profile', profile: 'profile' }, { ...emptyTurnInput, onChunk: () => started() });
    await ready;
    const pid = Number(readFileSync(count + '.pid', 'utf8'));
    backend.dispose();
    process.kill(pid, 0);
    assert.equal((await turn).reply, 'active-turn');
    let stopped = false;
    for (let attempt = 0; attempt < 30 && !stopped; attempt++) {
      await new Promise(resolve => setTimeout(resolve, 10));
      try { process.kill(pid, 0); } catch { stopped = true; }
    }
    assert.equal(stopped, true, 'retired child exits after the turn finishes');
  } finally {
    backend.dispose();
    rmSync(fake.dir, { recursive: true, force: true });
  }
});

test('each cached Claude connection keeps the runtime provenance of its own spawn', async () => {
  const fake = fakeAcpScript();
  const installed = join(fake.dir, 'bin');
  mkdirSync(installed);
  writeFileSync(join(installed, 'claude'), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
  // The real executable lookup and spawn env, with the fake ACP child in
  // place of the bridge so the model list reflects what this spawn ran.
  let searchPath = '';
  const backend = createAcpBackend('Claude', () => ({
    ...defaultClaudeSpawn(searchPath), command: process.execPath, args: [fake.path, 'runtime-models']
  }));
  const provenance = async (profile: string) => {
    const summary = await backend.listModels({ key: profile, profile });
    return { bundledRuntime: summary.bundledRuntime, model: summary.currentModelId };
  };
  const bundled = { bundledRuntime: true, model: 'bundled' };
  const cli = { bundledRuntime: false, model: 'installed' };
  try {
    assert.deepEqual(await provenance('a'), bundled);
    searchPath = [fake.dir, installed].join(delimiter);
    assert.deepEqual(await provenance('b'), cli);
    assert.deepEqual(await provenance('a'), bundled, 'A still runs the bundled binary it was spawned with');

    searchPath = fake.dir;
    assert.deepEqual(await provenance('c'), bundled);
    assert.deepEqual(await provenance('b'), cli, 'B still runs the installed CLI it was spawned with');
    assert.deepEqual(await provenance('a'), bundled);
  } finally {
    backend.dispose();
    rmSync(fake.dir, { recursive: true, force: true });
  }
});


test('model invalidation keeps a live ACP prompt steerable and cancellable', { timeout: 5000 }, async () => {
  const fake = fakeAcpScript();
  const count = join(fake.dir, 'count');
  const backend = createAcpBackend('Codex', () => ({ command: process.execPath, args: [fake.path, 'refresh-cancel', count] }));
  const adapter = resolveAdapter('codex');
  const original = adapter.invalidateModels;
  adapter.invalidateModels = backend.invalidateModels;
  try {
    let started!: () => void;
    const ready = new Promise<void>(resolve => { started = resolve; });
    const turn = backend.runTurn({ key: 'profile', profile: 'profile' }, { ...emptyTurnInput, onChunk: () => started() });
    await ready;
    invalidateAgentModels('codex');
    assert.equal(resolveAdapter('codex'), adapter);
    assert.deepEqual(await backend.steerTurn('profile', 'continue'), { outcome: 'injected' });
    await backend.cancelTurn('profile');
    await turn;
    assert.equal(readFileSync(count + '.steered', 'utf8'), 'yes');
    assert.equal(readFileSync(count + '.cancelled', 'utf8'), 'yes');
    await backend.listModels({ key: 'profile', profile: 'profile' });
    assert.equal(readFileSync(count, 'utf8'), '2', 'next catalog query starts an updated connection');
  } finally {
    adapter.invalidateModels = original;
    backend.dispose();
    rmSync(fake.dir, { recursive: true, force: true });
  }
});
