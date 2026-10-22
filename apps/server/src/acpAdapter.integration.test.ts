import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { test } from 'node:test';

import { createAcpBackend } from './managerChat/acpAdapter.js';
import { defaultClaudeSpawn } from './managerChat/registry.js';

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
const lines = readline.createInterface({ input: process.stdin });

function respond(message) {
  process.stdout.write(JSON.stringify(message) + '\n');
}

lines.on('line', (line) => {
  const request = JSON.parse(line);
  if (request.method === 'initialize') {
    respond({ jsonrpc: '2.0', id: request.id, result: { protocolVersion: request.params.protocolVersion } });
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
  if (request.method === 'session/new') {
    respond({ jsonrpc: '2.0', id: request.id, result: { sessionId: 'fake-session' } });
    return;
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
    backend.runTurn('profile', emptyTurnInput),
    /Hermes is not installed on this machine: the ".*gah-acp-command-that-does-not-exist" command was not found\./
  );
  // The failed connection must not be cached: the next turn tries again.
  await assert.rejects(backend.runTurn('profile', emptyTurnInput), /Hermes is not installed/);
});

test('detail-less ACP rejection surfaces code and bounded child diagnostics', async () => {
  const fake = fakeAcpScript();
  try {
    const backend = createAcpBackend('Codex', () => ({
      command: process.execPath,
      args: [fake.path, 'detail-less-error']
    }));

    await assert.rejects(
      backend.runTurn('profile', emptyTurnInput),
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
      backend.runTurn('profile', emptyTurnInput),
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
      backend.runTurn('profile', { ...emptyTurnInput, prompt: 'fails once', history: durableHistory }),
      /consecutive failures=1\/2/
    );
    const recovered = await backend.runTurn('profile', {
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
      backend.runTurn('profile', { ...emptyTurnInput, prompt: 'fails after success', history: currentHistory }),
      /consecutive failures=1\/2/
    );
    await assert.rejects(
      backend.runTurn('profile', { ...emptyTurnInput, prompt: 'second consecutive failure', history: currentHistory }),
      /consecutive failures=2\/2; connection evicted/
    );

    const rehydrated = await backend.runTurn('profile', {
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
    const stuck = backend.runTurn('profile', {
      ...emptyTurnInput,
      onChunk: (text) => { if (text === 'stuck-prompt-started') markPromptStarted(); }
    });
    await promptStarted;
    await backend.cancelTurn('profile');
    await assert.rejects(stuck);

    const recovered = await backend.runTurn('profile', emptyTurnInput);
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
    const result = await backend.runTurn('profile', emptyTurnInput);
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
    const turn = backend.runTurn('profile', { ...emptyTurnInput, onChunk: () => started() });
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
    const summary = await backend.listModels(profile);
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
