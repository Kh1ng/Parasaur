import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execPath } from 'node:process';
import type { ChatTranscriptTurn } from '@git-agent-harness/contracts';
import { agyBackendSpec, createHeadlessBackend, decodeVibeToolRequest, openhandsBackendSpec, parseOpenhandsReply, vibeBackendSpec, type HeadlessBackendSpec } from './headlessAdapter.js';

/** A fake one-shot CLI: echoes its cwd marker file's content so the test
 * proves the turn ran in the session cwd, and echoes the prompt tail. The
 * prompt arrives over stdin (issue #1009), never argv. */
function fakeCli(dir: string): string {
  const path = join(dir, 'fake-backend');
  writeFileSync(path, `#!/bin/sh
if [ -f ./MARKER ]; then echo "cwd-marker: $(cat ./MARKER)"; fi
echo "full-prompt: $(cat)"
`, { mode: 0o755 });
  return path;
}

/** History big enough that, replayed into one prompt, it would blow past a
 * conservative OS ARG_MAX (a few hundred KB) if it ever reached argv. */
function oversizedHistory(canary: string): ChatTranscriptTurn[] {
  const filler = 'x'.repeat(2000);
  const turns: ChatTranscriptTurn[] = [];
  for (let i = 0; i < 2000; i++) {
    turns.push({ role: 'user', text: `${filler}-${i}`, timestamp: i });
  }
  turns.push({ role: 'assistant', text: canary, timestamp: turns.length });
  return turns; // ~4MB of history, well past any conservative ARG_MAX
}

test('a headless turn runs in the conversation cwd and replays history into the prompt', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-'));
  const workdir = mkdtempSync(join(tmpdir(), 'gah-headless-wt-'));
  try {
    const cli = fakeCli(dir);
    const spec: HeadlessBackendSpec = {
      id: 'fake',
      displayName: 'Fake',
      turnArgs: () => [cli],
      encodeStdin: (prompt) => prompt
    };
    const backend = createHeadlessBackend(spec);

    writeFileSync(join(workdir, 'MARKER'), 'session-worktree');

    const history = [
      { role: 'user' as const, text: 'earlier question', timestamp: 1 },
      { role: 'assistant' as const, text: 'earlier answer', timestamp: 2 }
    ];
    const result = await backend.runTurn('p#s1', {
      prompt: 'new question',
      history,
      onChunk: () => {},
      onToolResult: () => {},
      cwd: workdir
    });

    assert.match(result.reply, /cwd-marker: session-worktree/, 'ran in the session cwd');
    assert.match(result.reply, /new question/, 'the new prompt is present');
    assert.match(result.reply, /earlier question/, 'prior history was replayed');
    assert.match(result.reply, /user: new question/, 'replay format matches resumePrompt');
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(workdir, { recursive: true, force: true });
  }
});

test('a headless turn spawns the backend with an allow-listed env plus the spec env', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-env-'));
  const saved = { COORDINATOR_TOKEN: process.env.COORDINATOR_TOKEN, TDAI_GATEWAY_API_KEY: process.env.TDAI_GATEWAY_API_KEY };
  try {
    process.env.COORDINATOR_TOKEN = 'owner-token';
    process.env.TDAI_GATEWAY_API_KEY = 'gateway-key';
    const cli = join(dir, 'env-backend');
    writeFileSync(cli, `#!/bin/sh
cat >/dev/null
echo "coordinator=\${COORDINATOR_TOKEN-unset} gateway=\${TDAI_GATEWAY_API_KEY-unset} adapter=\${ADAPTER_KEY-unset} home=\${HOME-unset} path=\${PATH:+set}"
`, { mode: 0o755 });
    const backend = createHeadlessBackend({
      id: 'fake',
      displayName: 'Fake',
      turnArgs: () => [cli],
      encodeStdin: (prompt) => prompt,
      spawnEnv: async () => ({ ADAPTER_KEY: 'adapter-key' })
    });
    const result = await backend.runTurn('p#env', { prompt: 'hi', history: [], onChunk: () => {}, onToolResult: () => {} });
    assert.equal(result.reply, `coordinator=unset gateway=unset adapter=adapter-key home=${process.env.HOME} path=set`);
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a headless backend surfaces CLI failure text and rejects cleanly', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-fail-'));
  try {
    const cli = join(dir, 'failing');
    writeFileSync(cli, '#!/bin/sh\ncat >/dev/null\necho "boom: bad credentials" >&2\nexit 3\n', { mode: 0o755 });
    const backend = createHeadlessBackend({
      id: 'fake',
      displayName: 'Fake',
      turnArgs: () => [cli],
      encodeStdin: (prompt) => prompt
    });
    await assert.rejects(
      backend.runTurn('p#s2', {
        prompt: 'hi',
        history: [],
        onChunk: () => {},
        onToolResult: () => {}
      }),
      /boom: bad credentials/
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a successful exit with empty trimmed stdout surfaces as an explicit error, not a silent empty reply', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-empty-'));
  try {
    const cli = join(dir, 'empty-output');
    writeFileSync(cli, '#!/bin/sh\ncat >/dev/null\necho "   "\nexit 0\n', { mode: 0o755 });
    const backend = createHeadlessBackend({
      id: 'fake',
      displayName: 'Fake',
      turnArgs: () => [cli],
      encodeStdin: (prompt) => prompt
    });
    await assert.rejects(
      backend.runTurn('p#s3', {
        prompt: 'hi',
        history: [],
        onChunk: () => {},
        onToolResult: () => {}
      }),
      /no output/
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('cancelTurn kills the in-flight process and the turn rejects, leaving the session recoverable', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-cancel-'));
  try {
    // A single Node child (no shell, no grandchildren) so SIGTERM's default
    // disposition — immediate termination — is deterministic and doesn't
    // depend on shell exec-optimization or orphaned descendants holding
    // stdio pipes open.
    let cli = [execPath, '-e', 'setTimeout(() => {}, 30000)'];
    const backend = createHeadlessBackend({
      id: 'fake',
      displayName: 'Fake',
      turnArgs: () => cli,
      encodeStdin: (prompt) => prompt
    });

    const turnPromise = backend.runTurn('p#cancel', {
      prompt: 'hi',
      history: [],
      onChunk: () => {},
      onToolResult: () => {}
    });
    await new Promise((resolve) => setTimeout(resolve, 200)); // let the child spawn
    await backend.cancelTurn('p#cancel');
    await assert.rejects(turnPromise);

    // The same backend/session is recoverable: swap in a real command and
    // run a second turn on the same profile key, proving cancelTurn cleared
    // state.child rather than leaving the session wedged.
    cli = [fakeCli(dir)];
    const result = await backend.runTurn('p#cancel', { prompt: 'again', history: [], onChunk: () => {}, onToolResult: () => {} });
    assert.match(result.reply, /again/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('headless backends advertise no config options and refuse config changes', async () => {
  const backend = createHeadlessBackend({
    id: 'fake',
    displayName: 'Fake',
    turnArgs: () => ['true'],
    encodeStdin: (prompt) => prompt
  });
  assert.deepEqual(await backend.listCommands('p'), []);
  const models = await backend.listModels('p');
  assert.equal(models.models.length, 0);
  assert.equal(models.reasoningEfforts.length, 0);
  await assert.rejects(backend.setModel('p', 'x'), /headless mode/);
  await assert.rejects(backend.setReasoningEffort('p', 'high'), /headless mode/);
});

test('AGY advertises its live model catalog and native reasoning efforts', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-agy-models-'));
  const originalPath = process.env.PATH;
  try {
    writeFileSync(join(dir, 'agy'), `#!/bin/sh
cat >/dev/null
printf 'Fetching available models...\\ngemini-3.7-flash-high\\tGemini 3.7 Flash (High)\\nclaude-sonnet-4-6\\tClaude Sonnet 4.6 (Thinking)\\n'
`, { mode: 0o755 });
    process.env.PATH = `${dir}:${originalPath}`;

    const backend = createHeadlessBackend(agyBackendSpec());
    const summary = await backend.listModels('p');
    assert.deepEqual(summary.models, [
      { id: 'gemini-3.7-flash-high', name: 'Gemini 3.7 Flash (High)' },
      { id: 'claude-sonnet-4-6', name: 'Claude Sonnet 4.6 (Thinking)' }
    ]);
    assert.deepEqual(summary.reasoningEfforts, [
      { id: 'low', name: 'Low' },
      { id: 'medium', name: 'Medium' },
      { id: 'high', name: 'High' }
    ]);
    await backend.setModel('p', 'claude-sonnet-4-6');
    await backend.setReasoningEffort('p', 'high');
    const selected = await backend.listModels('p');
    assert.equal(selected.currentModelId, 'claude-sonnet-4-6');
    assert.equal(selected.currentReasoningEffortId, 'high');
    await assert.rejects(backend.setModel('p', 'missing'), /Unknown model/);
    await assert.rejects(backend.setReasoningEffort('p', 'ultra'), /Unknown reasoning effort/);
  } finally {
    process.env.PATH = originalPath;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('AGY argv is fixed and content-free; the prompt travels over stdin as stream-json', () => {
  const spec = agyBackendSpec();
  assert.deepEqual(spec.turnArgs(), [
    'agy',
    '--input-format',
    'stream-json',
    '--output-format',
    'stream-json',
    '--dangerously-skip-permissions',
    '--sandbox'
  ]);
  assert.equal(
    spec.encodeStdin('three-turn remembered fact'),
    `${JSON.stringify({ event: 'user', message: { content: [{ type: 'text', text: 'three-turn remembered fact' }] } })}\n`
  );
});

test('Vibe argv is fixed and content-free regardless of prompt; the prompt travels over stdin, never sys.argv from exec', () => {
  const spec = vibeBackendSpec({ resolveInterpreter: () => '/fake/python3' });
  const args = spec.turnArgs();
  assert.deepEqual(args.slice(0, 2), ['/fake/python3', '-c']);
  assert.equal(args.length, 3, 'interpreter, -c, and one fixed bridge script — nothing else');
  assert.match(args[2], /sys\.argv/, 'the bridge sets sys.argv in-process rather than relying on exec argv');
  assert.equal(spec.encodeStdin('anything, arbitrarily large'), 'anything, arbitrarily large');
});

test('OpenHands reads the replayed prompt from /dev/stdin and receives profile credentials only through env', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-openhands-'));
  const originalPath = process.env.PATH;
  try {
    const record = join(dir, 'record.json');
    writeFileSync(join(dir, 'openhands'), `#!/bin/sh
node -e 'const fs=require("fs"); fs.writeFileSync(process.argv[1], JSON.stringify({argv:process.argv.slice(2), stdin:fs.readFileSync(0,"utf8"), model:process.env.LLM_MODEL, key:process.env.LLM_API_KEY}))' "${record}" "$@"
echo '{"source":"agent","kind":"MessageEvent","llm_message":{"content":[{"type":"text","text":"openhands reply"}]}}'
`, { mode: 0o755 });
    process.env.PATH = `${dir}:${originalPath}`;
    const backend = createHeadlessBackend(openhandsBackendSpec({
      resolveEnv: async () => ({ LLM_MODEL: 'profile-model', LLM_API_KEY: 'profile-key', LLM_BASE_URL: 'http://profile' })
    }));
    const result = await backend.runTurn('repo#openhands', {
      prompt: 'new question',
      history: [{ role: 'user', text: 'old question', timestamp: 1 }],
      onChunk: () => {},
      onToolResult: () => {}
    });
    const captured = JSON.parse(readFileSync(record, 'utf8')) as { argv: string[]; stdin: string; model: string; key: string };
    assert.equal(result.reply, 'openhands reply');
    assert.deepEqual(captured.argv, openhandsBackendSpec().turnArgs().slice(1));
    assert.match(captured.stdin, /old question/);
    assert.match(captured.stdin, /new question/);
    assert.equal(captured.model, 'profile-model');
    assert.equal(captured.key, 'profile-key');
    assert.ok(!captured.argv.join(' ').includes('question'));
  } finally {
    process.env.PATH = originalPath;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('OpenHands JSONL parser accepts finish actions and rejects tool-only output', () => {
  assert.equal(parseOpenhandsReply({
    stdout: '{"source":"agent","tool_name":"finish","action":{"message":"finished"}}\n',
    stderr: '',
    exitCode: 0
  }), 'finished');
  assert.throws(() => parseOpenhandsReply({
    stdout: '{"source":"tool","content":"secret tool output"}\n',
    stderr: '',
    exitCode: 0
  }), /no assistant reply/);
});

test('Vibe has no model/effort flag, so a session pin never reaches argv — it persists through the config instead (#1032 reopened, #1510)', () => {
  const spec = vibeBackendSpec({ resolveInterpreter: () => '/fake/python3' });
  const unpinned = spec.turnArgs();
  const pinned = spec.turnArgs({ model: 'claude-sonnet-4.6-thinking', reasoningEffort: 'high' });
  assert.deepEqual(pinned, unpinned, 'Vibe argv is unaffected by a session pin -- Vibe stays unchanged');
});

test('#1032 reopened: a pinned session reaches fake AGY as --model and --effort alongside the existing permission/sandbox flags', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-agy-pinned-'));
  const originalPath = process.env.PATH;
  try {
    const argvRecordPath = join(dir, 'argv.txt');
    const fakeAgy = join(dir, 'agy');
    writeFileSync(fakeAgy, `#!/bin/sh
printf '%s\\n' "$@" > "${argvRecordPath}"
cat >/dev/null
echo '{"event":"result","result":{"status":"SUCCESS","response":"agy-fake-reply"}}'
`, { mode: 0o755 });
    process.env.PATH = `${dir}:${originalPath}`;

    const backend = createHeadlessBackend(agyBackendSpec());
    const result = await backend.runTurn('p#agy-pinned', {
      prompt: 'question',
      history: [],
      onChunk: () => {},
      onToolResult: () => {},
      model: 'claude-sonnet-4.6-thinking',
      reasoningEffort: 'high'
    });

    assert.equal(result.reply, 'agy-fake-reply');
    const capturedArgv = readFileSync(argvRecordPath, 'utf8').trim().split('\n');
    assert.deepEqual(capturedArgv, [
      '--input-format',
      'stream-json',
      '--output-format',
      'stream-json',
      '--dangerously-skip-permissions',
      '--sandbox',
      '--model',
      'claude-sonnet-4.6-thinking',
      '--effort',
      'high'
    ]);
  } finally {
    process.env.PATH = originalPath;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('#1032 reopened: an unpinned session emits neither --model nor --effort to fake AGY', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-agy-unpinned-'));
  const originalPath = process.env.PATH;
  try {
    const argvRecordPath = join(dir, 'argv.txt');
    const fakeAgy = join(dir, 'agy');
    writeFileSync(fakeAgy, `#!/bin/sh
printf '%s\\n' "$@" > "${argvRecordPath}"
cat >/dev/null
echo '{"event":"result","result":{"status":"SUCCESS","response":"agy-fake-reply"}}'
`, { mode: 0o755 });
    process.env.PATH = `${dir}:${originalPath}`;

    const backend = createHeadlessBackend(agyBackendSpec());
    await backend.runTurn('p#agy-unpinned', {
      prompt: 'question',
      history: [],
      onChunk: () => {},
      onToolResult: () => {}
    });

    const capturedArgv = readFileSync(argvRecordPath, 'utf8');
    assert.ok(!capturedArgv.includes('--model'), 'no model pin means no --model flag');
    assert.ok(!capturedArgv.includes('--effort'), 'no effort pin means no --effort flag');
  } finally {
    process.env.PATH = originalPath;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('an oversized accumulated AGY prompt reaches the backend via stdin while argv stays a handful of fixed bytes', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-agy-oversized-'));
  const originalPath = process.env.PATH;
  try {
    const canary = 'CANARY_AGY_OVERSIZED_1009';
    const argvRecordPath = join(dir, 'argv.txt');
    const stdinRecordPath = join(dir, 'stdin.txt');
    const fakeAgy = join(dir, 'agy');
    writeFileSync(fakeAgy, `#!/bin/sh
printf '%s\\n' "$@" > "${argvRecordPath}"
cat > "${stdinRecordPath}"
echo '{"event":"result","result":{"status":"SUCCESS","response":"agy-fake-reply"}}'
`, { mode: 0o755 });
    process.env.PATH = `${dir}:${originalPath}`;

    const backend = createHeadlessBackend(agyBackendSpec());
    const result = await backend.runTurn('p#agy-oversized', {
      prompt: 'final question',
      history: oversizedHistory(canary),
      onChunk: () => {},
      onToolResult: () => {}
    });

    assert.equal(result.reply, 'agy-fake-reply');
    const capturedArgv = readFileSync(argvRecordPath, 'utf8');
    const capturedStdin = readFileSync(stdinRecordPath, 'utf8');
    assert.ok(!capturedArgv.includes(canary), 'the OS-level argv must never carry conversation content');
    assert.ok(capturedArgv.length < 1000, `argv stayed bounded (${capturedArgv.length} bytes) regardless of ~4MB of history`);
    assert.ok(capturedStdin.includes(canary), 'the prompt reached the backend over stdin');
  } finally {
    process.env.PATH = originalPath;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('an oversized accumulated Vibe prompt reaches the backend via stdin while argv stays a handful of fixed bytes', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-oversized-'));
  try {
    const canary = 'CANARY_VIBE_OVERSIZED_1009';
    const argvRecordPath = join(dir, 'argv.txt');
    const stdinRecordPath = join(dir, 'stdin.txt');
    const fakeInterpreter = join(dir, 'fake-python3');
    writeFileSync(fakeInterpreter, `#!/bin/sh
printf '%s\\n' "$@" > "${argvRecordPath}"
cat > "${stdinRecordPath}"
echo 'vibe-fake-reply'
`, { mode: 0o755 });

    const spec = vibeBackendSpec({ resolveInterpreter: () => fakeInterpreter });
    const backend = createHeadlessBackend(spec);
    const result = await backend.runTurn('p#vibe-oversized', {
      prompt: 'final question',
      history: oversizedHistory(canary),
      onChunk: () => {},
      onToolResult: () => {}
    });

    assert.equal(result.reply, 'vibe-fake-reply');
    const capturedArgv = readFileSync(argvRecordPath, 'utf8');
    const capturedStdin = readFileSync(stdinRecordPath, 'utf8');
    assert.ok(!capturedArgv.includes(canary), 'the OS-level argv must never carry conversation content');
    assert.ok(capturedArgv.length < 1000, `argv stayed bounded (${capturedArgv.length} bytes) regardless of ~4MB of history`);
    assert.ok(capturedStdin.includes(canary), 'the prompt reached the backend over stdin, where the bridge sets sys.argv in-process');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** A fake vibe interpreter that leaks the observed #1041 wire shape on its
 * first invocation, then — once the continuation prompt carries the read
 * file's content — emits the review text. Each prompt is recorded to its
 * own file so tests assert exactly what each invocation received. */
function fakeVibeInterpreter(dir: string, targetMarker: string): string {
  const path = join(dir, 'fake-python3');
  const firstStdin = join(dir, 'stdin-1.txt');
  const secondStdin = join(dir, 'stdin-2.txt');
  writeFileSync(path, `#!/bin/sh
prompt=$(cat)
case "$prompt" in
  *${targetMarker}*)
    printf '%s' "$prompt" > "${secondStdin}"
    echo 'Review complete: the target file is intact.'
    ;;
  *)
    printf '%s' "$prompt" > "${firstStdin}"
    printf 'read_file죰{"file_path": "target.md"}'
    ;;
esac
`, { mode: 0o755 });
  return path;
}

test('decodeVibeToolRequest parses the observed U+C8F0 wire shape and rejects undecodable requests', () => {
  const observed = 'read_file죰{"file_path": "/path/to/apps/server/src/managerChat/memoryGatewayClient.ts"}';
  assert.deepEqual(decodeVibeToolRequest(observed), {
    name: 'read_file',
    args: { file_path: '/path/to/apps/server/src/managerChat/memoryGatewayClient.ts' },
    raw: observed
  });
  assert.equal(decodeVibeToolRequest('an ordinary assistant reply'), null);
  assert.equal(decodeVibeToolRequest(''), null);
  assert.equal(decodeVibeToolRequest('text before read_file죰{"file_path": "x"}'), null, 'only a bare tool request decodes');
  assert.equal(decodeVibeToolRequest('read_file죰{"file_path": "x"} trailing text'), null);
  assert.throws(() => decodeVibeToolRequest('read_file죰{"file_path": }'), /not valid JSON/);
  assert.equal(decodeVibeToolRequest('read_file죰{"file_path": '), null, 'truncated syntax without a closing brace is not a decodable request');
});

test('only vibe declares a tool-request decoder; agy is untouched', () => {
  assert.equal(vibeBackendSpec({ resolveInterpreter: () => '/fake/python3' }).decodeToolRequest?.('read_file죰{}')?.name, 'read_file');
  assert.equal(agyBackendSpec().decodeToolRequest, undefined);
});

test('a leaked Vibe tool request is decoded, permission-gated, executed, and the turn continues with the result replayed', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-tool-'));
  const workdir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-wt-'));
  try {
    writeFileSync(join(workdir, 'target.md'), 'VIBE-TARGET-CONTENT-1041');
    const interpreter = fakeVibeInterpreter(dir, 'VIBE-TARGET-CONTENT-1041');
    const backend = createHeadlessBackend(vibeBackendSpec({ resolveInterpreter: () => interpreter }));

    const toolCalls: { toolCallId: string; name: string | null; status: string; locations: string[]; summary: string | null }[] = [];
    const toolResults: { name: string; text: string }[] = [];
    const result = await backend.runTurn('p#vibe-tool', {
      prompt: 'review target.md',
      history: [],
      onChunk: () => {},
      onToolResult: (name, text) => toolResults.push({ name, text }),
      cwd: workdir,
      onToolCall: (tool) => toolCalls.push(tool),
      requestPermission: async (request) => {
        assert.deepEqual(request.locations, [join(workdir, 'target.md')]);
        return 'allow-once';
      }
    });

    // The review continues after the read: the final reply is the
    // continuation's output, never the raw tool syntax.
    assert.equal(result.reply, 'Review complete: the target file is intact.');
    assert.ok(!result.reply.includes('\u{C8F0}'));
    assert.deepEqual(toolCalls.map((tool) => tool.status), ['pending', 'completed']);
    assert.equal(toolCalls[0].name, 'read_file');
    assert.equal(toolCalls[0].toolCallId, toolCalls[1].toolCallId, 'one card per decoded request');
    assert.deepEqual(toolCalls[0].locations, [join(workdir, 'target.md')], 'locations resolve against the session cwd');
    assert.equal(toolCalls[1].summary, 'VIBE-TARGET-CONTENT-1041');
    assert.deepEqual(toolResults, [{ name: 'read_file', text: 'VIBE-TARGET-CONTENT-1041' }]);

    // The continuation prompt replays the exchange so the backend sees its
    // own request plus the tool result.
    const firstPrompt = readFileSync(join(dir, 'stdin-1.txt'), 'utf8');
    assert.ok(!firstPrompt.includes('read_file'), 'the first invocation only carries the replayed conversation');
    const secondPrompt = readFileSync(join(dir, 'stdin-2.txt'), 'utf8');
    assert.match(secondPrompt, /user: review target\.md/);
    assert.match(secondPrompt, /assistant: read_file죰\{"file_path": "target\.md"\}/);
    assert.match(secondPrompt, /tool: VIBE-TARGET-CONTENT-1041/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(workdir, { recursive: true, force: true });
  }
});

test('a declined vibe tool request fails the turn with an actionable error and never continues', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-decline-'));
  const workdir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-decline-wt-'));
  try {
    writeFileSync(join(workdir, 'target.md'), 'VIBE-TARGET-CONTENT-1041');
    const interpreter = fakeVibeInterpreter(dir, 'VIBE-TARGET-CONTENT-1041');
    const backend = createHeadlessBackend(vibeBackendSpec({ resolveInterpreter: () => interpreter }));

    const toolCalls: { status: string; summary: string | null }[] = [];
    await assert.rejects(
      backend.runTurn('p#vibe-decline', {
        prompt: 'review target.md',
        history: [],
        onChunk: () => {},
        onToolResult: () => {},
        cwd: workdir,
        onToolCall: (tool) => toolCalls.push({ status: tool.status, summary: tool.summary }),
        requestPermission: async () => 'reject-once'
      }),
      /declined \(reject-once\); the turn cannot continue/
    );
    assert.deepEqual(toolCalls.map((tool) => tool.status), ['pending', 'failed']);
    assert.match(toolCalls[1].summary ?? '', /declined/);
    assert.throws(() => readFileSync(join(dir, 'stdin-2.txt'), 'utf8'), /ENOENT/, 'no continuation invocation after a decline');
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(workdir, { recursive: true, force: true });
  }
});

test('a decoded tool request with no permission UI attached fails closed instead of executing', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-noperm-'));
  const workdir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-noperm-wt-'));
  try {
    writeFileSync(join(workdir, 'target.md'), 'VIBE-TARGET-CONTENT-1041');
    const interpreter = fakeVibeInterpreter(dir, 'VIBE-TARGET-CONTENT-1041');
    const backend = createHeadlessBackend(vibeBackendSpec({ resolveInterpreter: () => interpreter }));
    await assert.rejects(
      backend.runTurn('p#vibe-noperm', {
        prompt: 'review target.md',
        history: [],
        onChunk: () => {},
        onToolResult: () => {},
        cwd: workdir
      }),
      /permission request was cancelled/
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(workdir, { recursive: true, force: true });
  }
});

test('undecodable or unservable vibe tool requests end the turn with an actionable error, never raw text', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-badtool-'));
  const workdir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-badtool-wt-'));
  try {
    const runWithReply = (reply: string) => {
      const interpreter = join(dir, `fake-python3-${reply.replace(/\W/g, '')}`);
      writeFileSync(interpreter, `#!/bin/sh
cat >/dev/null
printf '${reply}'
`, { mode: 0o755 });
      return createHeadlessBackend(vibeBackendSpec({ resolveInterpreter: () => interpreter })).runTurn('p#vibe-badtool', {
        prompt: 'go',
        history: [],
        onChunk: () => {},
        onToolResult: () => {},
        cwd: workdir,
        requestPermission: async () => 'allow-once'
      });
    };
    await assert.rejects(
      runWithReply('rm_rf죰{}'),
      /requested tool "rm_rf", which Parasaur cannot execute .*Executable tools: read_file/
    );
    await assert.rejects(
      runWithReply('read_file죰{"file_path": }'),
      /arguments are not valid JSON/
    );
    await assert.rejects(
      runWithReply('read_file죰{}'),
      /without a valid "file_path" argument/
    );
    await assert.rejects(
      runWithReply('read_file죰{"file_path": "../outside.txt"}'),
      /resolves outside this conversation's working directory/
    );
    await assert.rejects(
      runWithReply('read_file죰{"file_path": "missing.md"}'),
      /the read failed/
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(workdir, { recursive: true, force: true });
  }
});

test('a vibe turn stops before servicing more than the bounded number of tool requests', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-loop-'));
  const workdir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-loop-wt-'));
  try {
    writeFileSync(join(workdir, 'target.md'), 'VIBE-TARGET-CONTENT-1041');
    const interpreter = join(dir, 'fake-python3');
    writeFileSync(interpreter, `#!/bin/sh
cat >/dev/null
printf 'read_file죰{"file_path": "target.md"}'
`, { mode: 0o755 });
    const backend = createHeadlessBackend(vibeBackendSpec({ resolveInterpreter: () => interpreter }));
    await assert.rejects(
      backend.runTurn('p#vibe-loop', {
        prompt: 'review target.md',
        history: [],
        onChunk: () => {},
        onToolResult: () => {},
        cwd: workdir,
        requestPermission: async () => 'allow-once'
      }),
      /more than 5 tool calls in a single turn/
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(workdir, { recursive: true, force: true });
  }
});


test('bound OpenHands profile supplies model metadata without requiring or returning its legacy key', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-bound-openhands-profile-'));
  const saved = { HOME: process.env.HOME, GAH_BINARY: process.env.GAH_BINARY,
    LLM_MODEL: process.env.LLM_MODEL, LLM_BASE_URL: process.env.LLM_BASE_URL };
  const cli = join(dir, 'gah');
  try {
    process.env.HOME = dir;
    process.env.GAH_BINARY = cli;
    delete process.env.LLM_MODEL;
    delete process.env.LLM_BASE_URL;
    writeFileSync(cli, '#!/bin/sh\n'+ "echo '{\"profiles\":{\"bound\":{\"oh_profile\":\"work\"}}}'\n", { mode: 0o755 });
    mkdirSync(join(dir, '.openhands', 'profiles'), { recursive: true });
    const profile = join(dir, '.openhands', 'profiles', 'work.json');
    writeFileSync(profile, JSON.stringify({ model: 'openai/gpt', base_url: 'https://source.example/v1' }));
    const spec = openhandsBackendSpec({ instanceCredential: true });
    assert.deepEqual(await spec.spawnEnv?.('bound'), { LLM_MODEL: 'openai/gpt', LLM_BASE_URL: 'https://source.example/v1', OPENHANDS_SUPPRESS_BANNER: '1' });
    writeFileSync(profile, JSON.stringify({ model: 'openai/gpt', base_url: 'https://source.example/v1', api_key: 'old-profile-key' }));
    assert.equal((await spec.spawnEnv?.('bound'))?.LLM_API_KEY, undefined);
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});


/** A fake `gah` for the OpenHands picker (#1512): binds the GAH profile to
 * OpenHands profile "work", whose file the test writes per case. */
function fakeOpenhandsGah(dir: string): string {
  const cli = join(dir, 'gah');
  writeFileSync(cli, '#!/bin/sh\ncat >/dev/null\necho \'{"profiles":{"work":{"oh_profile":"work"}}}\'\n', { mode: 0o755 });
  return cli;
}

function writeOpenhandsProfile(dir: string, profile: Record<string, unknown>): void {
  mkdirSync(join(dir, '.openhands', 'profiles'), { recursive: true });
  writeFileSync(join(dir, '.openhands', 'profiles', 'work.json'), JSON.stringify(profile));
}

/** Records each (url, Authorization) the listing asked for and answers with
 * the queued status/body — no real network. */
function recordingFetch(responses: Array<{ status: number; body: unknown }>): {
  fetchFn: typeof globalThis.fetch;
  requested: { url: string; authorization: string }[];
} {
  const requested: { url: string; authorization: string }[] = [];
  let index = 0;
  return {
    requested,
    fetchFn: async (url: string | URL | Request, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      requested.push({ url: String(url), authorization: headers.get('Authorization') ?? '' });
      const queued = responses[Math.min(index, responses.length - 1)];
      index += 1;
      return new Response(JSON.stringify(queued.body), { status: queued.status });
    }
  };
}

test('OpenHands lists the provider\'s models with the configured one marked, read from the profile provider (#1512)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-openhands-models-'));
  const saved = { HOME: process.env.HOME, GAH_BINARY: process.env.GAH_BINARY,
    LLM_MODEL: process.env.LLM_MODEL, LLM_BASE_URL: process.env.LLM_BASE_URL, LLM_API_KEY: process.env.LLM_API_KEY };
  try {
    process.env.HOME = dir;
    process.env.GAH_BINARY = fakeOpenhandsGah(dir);
    delete process.env.LLM_MODEL;
    delete process.env.LLM_BASE_URL;
    delete process.env.LLM_API_KEY;
    writeOpenhandsProfile(dir, { model: 'openai/gpt-4o', base_url: 'https://provider.example/v1', api_key: 'profile-key' });
    const { fetchFn, requested } = recordingFetch([
      { status: 200, body: { data: [{ id: 'openai/gpt-4o' }, { id: 'openai/o3' }, { id: 'anthropic/claude-sonnet-4.6' }] } }
    ]);
    const backend = createHeadlessBackend(openhandsBackendSpec({ fetchFn }));

    const summary = await backend.listModels('work');
    assert.deepEqual(summary.models, [
      { id: 'openai/gpt-4o', name: 'openai/gpt-4o' },
      { id: 'openai/o3', name: 'openai/o3' },
      { id: 'anthropic/claude-sonnet-4.6', name: 'anthropic/claude-sonnet-4.6' }
    ]);
    assert.equal(summary.currentModelId, 'openai/gpt-4o', 'the configured profile model is the one marked');
    assert.deepEqual(summary.reasoningEfforts, []);
    assert.deepEqual(requested, [{ url: 'https://provider.example/v1/models', authorization: 'Bearer profile-key' }],
      'a LiteLLM-style base_url already ending in /v1 lists at /models');

    // Choosing a model marks it in-memory without a config write.
    await backend.setModel('work', 'anthropic/claude-sonnet-4.6');
    assert.equal((await backend.listModels('work')).currentModelId, 'anthropic/claude-sonnet-4.6');
    await assert.rejects(backend.setModel('work', 'not-offered'), /Unknown model/);
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the OpenHands listing is optional: a provider failure or a missing key falls back to the configured model (#1512)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-openhands-models-fallback-'));
  const saved = { HOME: process.env.HOME, GAH_BINARY: process.env.GAH_BINARY,
    LLM_MODEL: process.env.LLM_MODEL, LLM_BASE_URL: process.env.LLM_BASE_URL, LLM_API_KEY: process.env.LLM_API_KEY };
  try {
    process.env.HOME = dir;
    process.env.GAH_BINARY = fakeOpenhandsGah(dir);
    delete process.env.LLM_MODEL;
    delete process.env.LLM_BASE_URL;
    delete process.env.LLM_API_KEY;
    writeOpenhandsProfile(dir, { model: 'openai/gpt-4o', base_url: 'https://bare-provider.example', api_key: 'profile-key' });
    const { fetchFn, requested } = recordingFetch([{ status: 503, body: { error: 'unavailable' } }]);
    const failing = createHeadlessBackend(openhandsBackendSpec({ fetchFn }));

    const summary = await failing.listModels('work');
    assert.deepEqual(summary.models, [{ id: 'openai/gpt-4o', name: 'openai/gpt-4o' }],
      'a failing listing leaves exactly the configured model selectable');
    assert.equal(summary.currentModelId, 'openai/gpt-4o');
    assert.deepEqual(requested, [{ url: 'https://bare-provider.example/v1/models', authorization: 'Bearer profile-key' }],
      'a bare-origin base_url gets the conventional /v1 prefix');

    // A profile without an api_key never makes a network call.
    writeOpenhandsProfile(dir, { model: 'openai/gpt-4o', base_url: 'https://provider.example/v1' });
    const { fetchFn: unused, requested: none } = recordingFetch([{ status: 200, body: { data: [] } }]);
    const keyless = await createHeadlessBackend(openhandsBackendSpec({ fetchFn: unused })).listModels('work');
    assert.deepEqual(none, []);
    assert.deepEqual(keyless.models, [{ id: 'openai/gpt-4o', name: 'openai/gpt-4o' }]);

    // The ambient LLM_MODEL a turn would actually run is the one marked.
    process.env.LLM_MODEL = 'openai/env-model';
    const envMarked = await failing.listModels('work');
    assert.equal(envMarked.currentModelId, 'openai/env-model');
    assert.ok(envMarked.models.some((model) => model.id === 'openai/env-model'));
    delete process.env.LLM_MODEL;
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a GAH profile with no bound OpenHands profile lists no models (#1512)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-openhands-models-unbound-'));
  const saved = { HOME: process.env.HOME, GAH_BINARY: process.env.GAH_BINARY };
  const cli = join(dir, 'gah');
  try {
    process.env.HOME = dir;
    process.env.GAH_BINARY = cli;
    writeFileSync(cli, '#!/bin/sh\ncat >/dev/null\necho \'{"profiles":{"work":{}}}\'\n', { mode: 0o755 });
    const summary = await createHeadlessBackend(openhandsBackendSpec()).listModels('work');
    assert.deepEqual(summary.models, []);
    assert.equal(summary.currentModelId, null);
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a credential-bound OpenHands instance lists only its configured model, never the profile key (#1512)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-openhands-models-instance-'));
  const saved = { HOME: process.env.HOME, GAH_BINARY: process.env.GAH_BINARY,
    LLM_MODEL: process.env.LLM_MODEL, LLM_BASE_URL: process.env.LLM_BASE_URL, LLM_API_KEY: process.env.LLM_API_KEY };
  try {
    process.env.HOME = dir;
    process.env.GAH_BINARY = fakeOpenhandsGah(dir);
    delete process.env.LLM_MODEL;
    delete process.env.LLM_BASE_URL;
    delete process.env.LLM_API_KEY;
    writeOpenhandsProfile(dir, { model: 'openai/gpt-4o', base_url: 'https://source.example/v1', api_key: 'old-profile-key' });
    const { fetchFn, requested } = recordingFetch([{ status: 200, body: { data: [{ id: 'openai/o3' }] } }]);
    const summary = await createHeadlessBackend(openhandsBackendSpec({ instanceCredential: true, fetchFn })).listModels('work');
    assert.deepEqual(requested, [], 'the instance\'s credential owns its provider endpoint and key; there is nothing here to list against');
    assert.deepEqual(summary.models, [{ id: 'openai/gpt-4o', name: 'openai/gpt-4o' }]);
    assert.equal(summary.currentModelId, 'openai/gpt-4o');
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test('choosing an OpenHands model is honored on the next turn as LLM_MODEL (#1512)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-openhands-setmodel-'));
  const saved = { HOME: process.env.HOME, GAH_BINARY: process.env.GAH_BINARY, PATH: process.env.PATH,
    LLM_MODEL: process.env.LLM_MODEL, LLM_BASE_URL: process.env.LLM_BASE_URL, LLM_API_KEY: process.env.LLM_API_KEY };
  try {
    process.env.HOME = dir;
    process.env.GAH_BINARY = fakeOpenhandsGah(dir);
    delete process.env.LLM_MODEL;
    delete process.env.LLM_BASE_URL;
    delete process.env.LLM_API_KEY;
    writeOpenhandsProfile(dir, { model: 'openai/gpt-4o', base_url: 'https://provider.example/v1', api_key: 'profile-key' });
    writeFileSync(join(dir, 'openhands'), `#!/bin/sh
node -e 'const fs=require("fs"); fs.writeFileSync(process.argv[1], JSON.stringify({model:process.env.LLM_MODEL}))' "${join(dir, 'record.json')}" "$@"
echo '{"source":"agent","kind":"MessageEvent","llm_message":{"content":[{"type":"text","text":"openhands reply"}]}}'
`, { mode: 0o755 });
    process.env.PATH = `${dir}:${saved.PATH}`;
    const { fetchFn } = recordingFetch([
      { status: 200, body: { data: [{ id: 'openai/gpt-4o' }, { id: 'openai/o3' }, { id: 'anthropic/claude-sonnet-4.6' }] } }
    ]);
    const backend = createHeadlessBackend(openhandsBackendSpec({ fetchFn }));
    const record = () => JSON.parse(readFileSync(join(dir, 'record.json'), 'utf8')) as { model: string };
    const turn = (model?: string | null) => backend.runTurn('work', {
      prompt: 'go', history: [], onChunk: () => {}, onToolResult: () => {}, ...(model !== undefined ? { model } : {})
    });

    // setModel pins the conversation; the next turn carries it as LLM_MODEL.
    await backend.setModel('work', 'anthropic/claude-sonnet-4.6');
    await turn();
    assert.equal(record().model, 'anthropic/claude-sonnet-4.6', 'the choice reached the OpenHands child ahead of the profile model');

    // An explicit per-turn model wins the same way, without a prior setModel.
    await turn('openai/o3');
    assert.equal(record().model, 'openai/o3');

    // An unpinned turn leaves the profile's configured model in place.
    await turn(null);
    assert.equal(record().model, 'openai/gpt-4o');
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the OpenHands listing reads base URL and key as one profile-only pair; ambient LLM_* never substitutes (#1512)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-openhands-models-profile-pair-'));
  const saved = { HOME: process.env.HOME, GAH_BINARY: process.env.GAH_BINARY,
    LLM_MODEL: process.env.LLM_MODEL, LLM_BASE_URL: process.env.LLM_BASE_URL, LLM_API_KEY: process.env.LLM_API_KEY };
  try {
    process.env.HOME = dir;
    process.env.GAH_BINARY = fakeOpenhandsGah(dir);
    delete process.env.LLM_MODEL;
    delete process.env.LLM_BASE_URL;
    delete process.env.LLM_API_KEY;
    writeOpenhandsProfile(dir, { model: 'openai/gpt-4o', base_url: 'https://profile-provider.example/v1', api_key: 'profile-key' });

    // An ambient LLM_BASE_URL must not become the listing target: the
    // profile key would otherwise leave for an unrelated endpoint.
    process.env.LLM_BASE_URL = 'https://ambient-endpoint.example/v1';
    const { fetchFn: urlFetch, requested: urlRequested } = recordingFetch([{ status: 200, body: { data: [{ id: 'openai/o3' }] } }]);
    const mixedUrl = await createHeadlessBackend(openhandsBackendSpec({ fetchFn: urlFetch })).listModels('work');
    assert.deepEqual(urlRequested, [{ url: 'https://profile-provider.example/v1/models', authorization: 'Bearer profile-key' }],
      'the listing is made only against the profile base_url, with the profile key');
    assert.ok(mixedUrl.models.some((model) => model.id === 'openai/o3'));

    // An ambient LLM_API_KEY must not become the listing credential: it
    // would otherwise be sent to the profile's endpoint.
    delete process.env.LLM_BASE_URL;
    process.env.LLM_API_KEY = 'ambient-key';
    const { fetchFn: keyFetch, requested: keyRequested } = recordingFetch([{ status: 200, body: { data: [{ id: 'openai/o3' }] } }]);
    const mixedKey = await createHeadlessBackend(openhandsBackendSpec({ fetchFn: keyFetch })).listModels('work');
    assert.deepEqual(keyRequested, [{ url: 'https://profile-provider.example/v1/models', authorization: 'Bearer profile-key' }],
      'the profile endpoint is asked only with the profile key, never the ambient one');
    assert.ok(mixedKey.models.some((model) => model.id === 'openai/o3'));

    // A profile missing either half does not list at all, even when the
    // ambient env could have supplied the missing half.
    process.env.LLM_BASE_URL = 'https://ambient-endpoint.example/v1';
    process.env.LLM_API_KEY = 'ambient-key';
    writeOpenhandsProfile(dir, { model: 'openai/gpt-4o', api_key: 'profile-key' });
    const { fetchFn: urllessFetch, requested: urllessRequested } = recordingFetch([{ status: 200, body: { data: [] } }]);
    const urlless = await createHeadlessBackend(openhandsBackendSpec({ fetchFn: urllessFetch })).listModels('work');
    assert.deepEqual(urllessRequested, [], 'a profile without base_url never lists, ambient LLM_BASE_URL notwithstanding');
    assert.deepEqual(urlless.models, [{ id: 'openai/gpt-4o', name: 'openai/gpt-4o' }]);

    writeOpenhandsProfile(dir, { model: 'openai/gpt-4o', base_url: 'https://profile-provider.example/v1' });
    const { fetchFn: keylessFetch, requested: keylessRequested } = recordingFetch([{ status: 200, body: { data: [] } }]);
    const keyless = await createHeadlessBackend(openhandsBackendSpec({ fetchFn: keylessFetch })).listModels('work');
    assert.deepEqual(keylessRequested, [], 'a profile without api_key never lists, ambient LLM_API_KEY notwithstanding');
    assert.deepEqual(keyless.models, [{ id: 'openai/gpt-4o', name: 'openai/gpt-4o' }]);
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test('every OpenHands listing failure logs one sanitized diagnostic, once per failing transition (#1512)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-openhands-models-diagnostics-'));
  const saved = { HOME: process.env.HOME, GAH_BINARY: process.env.GAH_BINARY,
    LLM_MODEL: process.env.LLM_MODEL, LLM_BASE_URL: process.env.LLM_BASE_URL, LLM_API_KEY: process.env.LLM_API_KEY };
  const originalWarn = console.warn;
  const warnings: string[] = [];
  console.warn = (message: string) => { warnings.push(message); };
  try {
    process.env.HOME = dir;
    process.env.GAH_BINARY = fakeOpenhandsGah(dir);
    delete process.env.LLM_MODEL;
    delete process.env.LLM_BASE_URL;
    delete process.env.LLM_API_KEY;
    writeOpenhandsProfile(dir, { model: 'openai/gpt-4o', base_url: 'https://profile-provider.example/v1', api_key: 'profile-key' });

    // A non-200 logs the status only — not the response body.
    const { fetchFn: unauthorizedFetch } = recordingFetch([{ status: 401, body: { error: 'bad profile-key at provider' } }]);
    const unauthorized = await createHeadlessBackend(openhandsBackendSpec({ fetchFn: unauthorizedFetch })).listModels('work');
    assert.deepEqual(unauthorized.models, [{ id: 'openai/gpt-4o', name: 'openai/gpt-4o' }]);
    assert.equal(warnings.length, 1);
    assert.match(warnings[0], /openhands model listing unavailable for work: http 401$/);
    assert.ok(!warnings[0].includes('profile-key') && !warnings[0].includes('provider'), 'no key or body text reaches the log');

    // The same failure repeating (the picker polls) stays quiet.
    await createHeadlessBackend(openhandsBackendSpec({ fetchFn: unauthorizedFetch })).listModels('work');
    assert.equal(warnings.length, 1, 'one line per failing transition, not per poll');

    // A malformed 200 logs the failure class, not the payload.
    const { fetchFn: malformedFetch } = recordingFetch([{ status: 200, body: 'not the models payload' }]);
    const malformed = await createHeadlessBackend(openhandsBackendSpec({ fetchFn: malformedFetch })).listModels('work');
    assert.deepEqual(malformed.models, [{ id: 'openai/gpt-4o', name: 'openai/gpt-4o' }]);
    assert.equal(warnings.length, 2);
    assert.match(warnings[1], /openhands model listing unavailable for work: malformed response$/);
    assert.ok(!warnings[1].includes('not the models payload'), 'the response body never reaches the log');

    // A thrown fetch logs the error class, not the message or stack.
    const thrownError = new TypeError('fetch failed against https://profile-provider.example/v1/models?token=profile-key');
    const throwing = createHeadlessBackend(openhandsBackendSpec({
      fetchFn: async () => { throw thrownError; }
    }));
    const thrown = await throwing.listModels('work');
    assert.deepEqual(thrown.models, [{ id: 'openai/gpt-4o', name: 'openai/gpt-4o' }]);
    assert.equal(warnings.length, 3);
    assert.match(warnings[2], /openhands model listing unavailable for work: TypeError$/);
    assert.ok(!warnings[2].includes('profile-key') && !warnings[2].includes('https://'), 'no URL or key text reaches the log');

    // A success clears the entry, so the same failure logs again on its
    // next failing transition (the wsServer once-per-transition shape).
    const { fetchFn: recoveringFetch } = recordingFetch([
      { status: 200, body: { data: [{ id: 'openai/o3' }] } },
      { status: 401, body: {} }
    ]);
    const recovering = createHeadlessBackend(openhandsBackendSpec({ fetchFn: recoveringFetch }));
    const recovered = await recovering.listModels('work');
    assert.equal(warnings.length, 3, 'a recovered listing logs nothing');
    assert.ok(recovered.models.some((model) => model.id === 'openai/o3'));
    await recovering.listModels('work');
    assert.equal(warnings.length, 4, 'a failure after recovery logs again');
    assert.match(warnings[3], /openhands model listing unavailable for work: http 401$/);
  } finally {
    console.warn = originalWarn;
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});


test('a named Vibe instance derives its Python bridge from the selected launcher', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-vibe-selected-launcher-'));
  const selected = join(dir, 'selected-vibe');
  try {
    writeFileSync(selected, '#!/selected/install/bin/python3\n');
    assert.equal(vibeBackendSpec({ executable: selected }).turnArgs()[0], '/selected/install/bin/python3');
    writeFileSync(selected, '#!/usr/bin/env python3\n');
    assert.throws(() => vibeBackendSpec({ executable: selected }).turnArgs(), /absolute Python interpreter/);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

/** A fake `gah` for the Vibe picker (#1510): `config show-vibe-models`
 * prints the models recorded in the config fixture with the active one
 * read from the state file, and `config set-vibe-model` records the choice
 * and the argv it arrived with — so tests prove the picker reads and
 * persists through the real CLI surface, not a shortcut. */
function fakeVibeGah(dir: string): { state: string; argv: string } {
  const state = join(dir, 'active-model.txt');
  const argv = join(dir, 'argv.txt');
  const cli = join(dir, 'gah');
  const config = JSON.stringify([
    { id: 'glm-5.3', name: 'GLM 5.3' },
    { id: 'mistral-medium-3.5', name: 'Mistral Medium 3.5' }
  ]);
  writeFileSync(state, 'mistral-medium-3.5');
  writeFileSync(cli, `#!/bin/sh
printf '%s\\n' "$@" > "${argv}"
case "$1 $2" in
  "config show-vibe-models")
    printf '{"models":%s,"active_model":"%s"}\\n' '${config.replace(/'/g, "'\\''")}' "$(cat "${state}")"
    ;;
  "config set-vibe-model")
    while [ $# -gt 1 ]; do
      if [ "$1" = "--model" ]; then printf '%s' "$2" > "${state}"; fi
      shift
    done
    ;;
  *) echo "unexpected gah invocation: $*" >&2; exit 1 ;;
esac
`, { mode: 0o755 });
  return { state, argv };
}

test('Vibe lists its configured models with the active one marked, read from its own config through the gah CLI (#1510)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-models-'));
  const saved = { GAH_BINARY: process.env.GAH_BINARY };
  try {
    const { argv } = fakeVibeGah(dir);
    process.env.GAH_BINARY = join(dir, 'gah');
    const backend = createHeadlessBackend(vibeBackendSpec());

    const summary = await backend.listModels('vibe-profile');
    assert.deepEqual(summary.models, [
      { id: 'glm-5.3', name: 'GLM 5.3' },
      { id: 'mistral-medium-3.5', name: 'Mistral Medium 3.5' }
    ]);
    assert.equal(summary.currentModelId, 'mistral-medium-3.5', 'the config\'s active model is marked');
    assert.deepEqual(summary.reasoningEfforts, []);
    const requested = readFileSync(argv, 'utf8').trim().split('\n');
    assert.ok(requested.includes('--profile') && requested.includes('vibe-profile'));
    assert.ok(!requested.includes('--instance'), 'the ambient installation is the default scope');

    await assert.rejects(backend.setModel('vibe-profile', 'not-configured'), /Unknown model/);
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test('choosing a Vibe model persists active_model through the gah CLI and the mark survives a fresh adapter (#1510)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-setmodel-'));
  const saved = { GAH_BINARY: process.env.GAH_BINARY };
  try {
    const { state, argv } = fakeVibeGah(dir);
    process.env.GAH_BINARY = join(dir, 'gah');
    const backend = createHeadlessBackend(vibeBackendSpec());

    await backend.setModel('vibe-profile', 'glm-5.3');
    assert.equal(readFileSync(state, 'utf8'), 'glm-5.3', 'the choice reached the config writer');
    const requested = readFileSync(argv, 'utf8').trim().split('\n');
    assert.deepEqual(
      requested.filter((_, index) => index >= requested.indexOf('set-vibe-model')),
      ['set-vibe-model', '--profile', 'vibe-profile', '--model', 'glm-5.3'],
      'the write is a fixed argv: profile, model, nothing else'
    );

    // A fresh adapter has no in-memory selection: the active mark must
    // come back from the persisted config.
    const reloaded = await createHeadlessBackend(vibeBackendSpec()).listModels('vibe-profile');
    assert.equal(reloaded.currentModelId, 'glm-5.3', 'the persisted active model is the one marked');
    assert.ok(reloaded.models.some((model) => model.id === 'glm-5.3'));
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a named Vibe instance scopes its model reads and writes to the instance (#1510)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-instance-'));
  const saved = { GAH_BINARY: process.env.GAH_BINARY };
  try {
    const { state, argv } = fakeVibeGah(dir);
    process.env.GAH_BINARY = join(dir, 'gah');
    const backend = createHeadlessBackend(vibeBackendSpec({ instance: 'mistral-tak-vibe' }));

    const summary = await backend.listModels('vibe-profile');
    assert.equal(summary.models.length, 2);
    let requested = readFileSync(argv, 'utf8').trim().split('\n');
    assert.ok(requested.includes('--instance') && requested.includes('mistral-tak-vibe'));

    await backend.setModel('vibe-profile', 'glm-5.3');
    assert.equal(readFileSync(state, 'utf8'), 'glm-5.3');
    requested = readFileSync(argv, 'utf8').trim().split('\n');
    assert.ok(requested.includes('--instance') && requested.includes('mistral-tak-vibe'), 'the write is scoped too');
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a pinned Vibe turn persists the pinned model before spawning, so the CLI picks it up from its config (#1510)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-pinned-'));
  const saved = { GAH_BINARY: process.env.GAH_BINARY };
  try {
    const { state } = fakeVibeGah(dir);
    process.env.GAH_BINARY = join(dir, 'gah');
    const interpreter = join(dir, 'fake-python3');
    writeFileSync(interpreter, '#!/bin/sh\ncat >/dev/null\necho vibe-reply\n', { mode: 0o755 });

    const backend = createHeadlessBackend(vibeBackendSpec({ resolveInterpreter: () => interpreter }));
    const result = await backend.runTurn('vibe-profile', {
      prompt: 'go',
      history: [],
      profile: 'vibe-profile',
      onChunk: () => {},
      onToolResult: () => {},
      model: 'glm-5.3'
    });

    assert.equal(result.reply, 'vibe-reply');
    assert.equal(readFileSync(state, 'utf8'), 'glm-5.3', 'the pin was persisted before the turn ran');

    // An unpinned turn never rewrites the config.
    writeFileSync(state, 'mistral-medium-3.5');
    await backend.runTurn('vibe-profile', {
      prompt: 'go again',
      history: [],
      profile: 'vibe-profile',
      onChunk: () => {},
      onToolResult: () => {},
      model: null
    });
    assert.equal(readFileSync(state, 'utf8'), 'mistral-medium-3.5', 'an unpinned session leaves the selection alone');
  } finally {
    for (const [name, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[name]; else process.env[name] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

/** The shared selection a config-backed (vibe-shaped) backend persists to
 * and reads at spawn time, like Vibe's child reads `active_model` from its
 * config at startup: one file per installation, shared by every
 * conversation using it. */
function sharedSelectionBackend(dir: string, options: { gate?: boolean } = {}): {
  backend: ReturnType<typeof createHeadlessBackend>;
  selection: string;
  gate: string;
} {
  const selection = join(dir, 'selection.txt');
  const gate = join(dir, 'spawn-gate');
  const interpreter = join(dir, 'fake-python3');
  writeFileSync(selection, 'none');
  // The child reports the selection it sees when the gate opens — the
  // gate stands in for a later spawn of the same turn (a tool
  // continuation), which must still observe this turn's own pin. The
  // wait is bounded so a broken test fails instead of hanging.
  const gated = options.gate
    ? `i=0\nwhile [ ! -f "${gate}" ] && [ "$i" -lt 500 ]; do sleep 0.02; i=$((i+1)); done\n`
    : '';
  writeFileSync(interpreter, `#!/bin/sh
cat >/dev/null
${gated}echo "ran-with: $(cat '${selection}')"
`, { mode: 0o755 });
  const spec: HeadlessBackendSpec = {
    id: 'vibe-like',
    displayName: 'Vibe-like',
    turnArgs: () => [interpreter],
    encodeStdin: (prompt) => prompt,
    spawnEnv: async () => { await Promise.resolve(); return {}; },
    modelOptions: async () => ({
      models: [{ id: 'model-A', name: 'A' }, { id: 'model-B', name: 'B' }],
      reasoningEfforts: [],
      currentModelId: null
    }),
    applyModel: async (_profile, model) => { writeFileSync(selection, model); },
    modelSelectionScope: () => join(dir, 'vibe-home')
  };
  return { backend: createHeadlessBackend(spec), selection, gate };
}

test('concurrent pinned turns on one installation serialize, so each turn runs with its own pinned model (#1510 round 2)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-serialize-'));
  try {
    const { backend } = sharedSelectionBackend(dir);
    const turn = (model: string, prompt: string) =>
      backend.runTurn(`p#conv-${model}`, {
        prompt,
        history: [],
        onChunk: () => {},
        onToolResult: () => {},
        model
      });

    const [turnA, turnB] = await Promise.all([turn('model-A', 'a'), turn('model-B', 'b')]);
    assert.match(turnA.reply, /model-A/, "turn A ran with A's pinned model");
    assert.match(turnB.reply, /model-B/, "turn B ran with B's pinned model");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a setModel write waits for an in-flight pinned turn, so the turn never runs another session\'s selection (#1510 round 2)', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-headless-vibe-setmodel-race-'));
  try {
    const { backend, selection, gate } = sharedSelectionBackend(dir, { gate: true });
    const turnPromise = backend.runTurn('p#conv-a', {
      prompt: 'a',
      history: [],
      onChunk: () => {},
      onToolResult: () => {},
      model: 'model-A'
    });
    const setPromise = backend.setModel('p#conv-b', 'model-B');
    // Let both contenders run: the pinned turn holds the installation's
    // mutex, so the write queues behind it instead of landing mid-turn.
    await new Promise(resolve => setImmediate(resolve));
    writeFileSync(gate, 'now');
    const result = await turnPromise;
    await setPromise;
    assert.match(result.reply, /model-A/, 'the gated spawn still saw the turn\'s own pin');
    assert.equal(readFileSync(selection, 'utf8'), 'model-B', 'the queued write landed after the turn completed');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('the Vibe spec scopes model serialization to its installation home (#1510 round 2)', () => {
  const saved = { VIBE_HOME: process.env.VIBE_HOME };
  try {
    process.env.VIBE_HOME = '/custom/vibe-home';
    assert.equal(vibeBackendSpec().modelSelectionScope?.(), '/custom/vibe-home');
    assert.equal(vibeBackendSpec({ instance: 'vibe-work', stateRoot: '/state/root' }).modelSelectionScope?.(), join('/state/root', '.vibe'));
  } finally {
    if (saved.VIBE_HOME === undefined) delete process.env.VIBE_HOME; else process.env.VIBE_HOME = saved.VIBE_HOME;
  }
});
