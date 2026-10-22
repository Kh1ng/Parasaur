import type { Session } from '@git-agent-harness/contracts';
import {
  MAX_FINISHED_SESSIONS,
  MAX_OUTPUT_CHARS_PER_STREAM,
  MAX_OUTPUT_LINES_PER_STREAM,
  MAX_RETAINED_SESSIONS,
  SessionOutputBuffer
} from './sessionOutputBuffer.js';
import { useSessionOutputStore } from './sessionOutputStore.js';

const session = (id: string, status: Session['status']): Session => ({ id, instanceId: 'claude-default', providerKind: 'claude', status });

test('10k stdout lines keep only the newest lines, flagged as truncated', () => {
  const buffer = new SessionOutputBuffer();
  for (let i = 0; i < 10_000; i += 1) buffer.append('s1', 'stdout', `line ${i}`);

  const lines = buffer.lines('s1', 'stdout');
  expect(lines).toHaveLength(MAX_OUTPUT_LINES_PER_STREAM);
  expect(lines[0]).toBe(`line ${10_000 - MAX_OUTPUT_LINES_PER_STREAM}`);
  expect(lines.at(-1)).toBe('line 9999');
  const text = buffer.text('s1')!;
  expect(text.truncated).toBe(true);
  expect(text.stdout.endsWith('line 9998\nline 9999\n')).toBe(true);
  expect(text.stderr).toBe('');
});

test('character cap bounds a stream of long lines and a single huge line', () => {
  const buffer = new SessionOutputBuffer();
  const long = 'x'.repeat(1000);
  for (let i = 0; i < 1000; i += 1) buffer.append('s1', 'stderr', `${i}:${long}`);
  const kept = buffer.lines('s1', 'stderr');
  expect(kept.join('').length).toBeLessThanOrEqual(MAX_OUTPUT_CHARS_PER_STREAM);
  expect(kept.at(-1)).toBe(`999:${long}`);

  buffer.append('s2', 'stdout', 'a'.repeat(MAX_OUTPUT_CHARS_PER_STREAM) + 'tail');
  const [only] = buffer.lines('s2', 'stdout');
  expect(only).toHaveLength(MAX_OUTPUT_CHARS_PER_STREAM);
  expect(only.endsWith('tail')).toBe(true);
  expect(buffer.text('s2')!.truncated).toBe(true);
});

test('lines dropped by the character cap are released, not just hidden', () => {
  const buffer = new SessionOutputBuffer();
  // Distinct large lines: small ones (many per budget) and ones near the cap,
  // enough of each to pass the line-count compaction threshold.
  const sizes = [4096, MAX_OUTPUT_CHARS_PER_STREAM / 4 + 1, MAX_OUTPUT_CHARS_PER_STREAM - 16];
  const count = MAX_OUTPUT_LINES_PER_STREAM + 500;
  for (const [s, size] of sizes.entries()) {
    const sessionId = `s${s}`;
    const fill = 'x'.repeat(size);
    for (let i = 0; i < count; i += 1) {
      buffer.append(sessionId, 'stdout', `${String(i).padStart(8, '0')}${fill}`.slice(0, size));
      const storage = buffer.storageForTest(sessionId, 'stdout')!;
      expect(storage.chars).toBeLessThanOrEqual(MAX_OUTPUT_CHARS_PER_STREAM);
      expect(storage.slots).toBeLessThanOrEqual(2 * MAX_OUTPUT_LINES_PER_STREAM);
    }
    const kept = buffer.lines(sessionId, 'stdout');
    expect(kept.at(-1)!.startsWith(String(count - 1).padStart(8, '0'))).toBe(true);
    expect(kept.join('').length).toBe(buffer.storageForTest(sessionId, 'stdout')!.chars);
    expect(buffer.text(sessionId)!.truncated).toBe(true);
  }
});

test('short output is kept verbatim in the previous line format', () => {
  const buffer = new SessionOutputBuffer();
  buffer.append('s1', 'stdout', 'one');
  buffer.append('s1', 'stderr', 'warn');
  buffer.append('s1', 'stdout', 'two');
  expect(buffer.text('s1')).toEqual({ stdout: 'one\ntwo\n', stderr: 'warn\n', truncated: false });
  expect(buffer.text('missing')).toBeUndefined();
});

test('finished sessions are kept only for the most recent MAX_FINISHED_SESSIONS', () => {
  const buffer = new SessionOutputBuffer();
  buffer.append('running', 'stdout', 'still going');
  for (let i = 0; i <= MAX_FINISHED_SESSIONS; i += 1) {
    buffer.append(`done-${i}`, 'stdout', 'bye');
    buffer.markFinished(`done-${i}`);
  }
  expect(buffer.has('done-0')).toBe(false);
  expect(buffer.has('done-1')).toBe(true);
  expect(buffer.has(`done-${MAX_FINISHED_SESSIONS}`)).toBe(true);
  expect(buffer.has('running')).toBe(true);
});

test('a resumed session is no longer subject to finished eviction', () => {
  const buffer = new SessionOutputBuffer();
  buffer.append('resumed', 'stdout', 'x');
  buffer.markFinished('resumed');
  buffer.markActive('resumed');
  for (let i = 0; i <= MAX_FINISHED_SESSIONS; i += 1) {
    buffer.append(`done-${i}`, 'stdout', 'bye');
    buffer.markFinished(`done-${i}`);
  }
  expect(buffer.has('resumed')).toBe(true);
});

test('sessions missing from a replaced list are treated as finished', () => {
  const buffer = new SessionOutputBuffer();
  for (let i = 0; i <= MAX_FINISHED_SESSIONS; i += 1) buffer.append(`gone-${i}`, 'stdout', 'x');
  buffer.append('live', 'stdout', 'x');
  buffer.retainLive(['live']);
  expect(buffer.has('gone-0')).toBe(false);
  expect(buffer.has(`gone-${MAX_FINISHED_SESSIONS}`)).toBe(true);
  expect(buffer.has('live')).toBe(true);
});

test('the total number of retained sessions is capped', () => {
  const buffer = new SessionOutputBuffer();
  for (let i = 0; i < MAX_RETAINED_SESSIONS + 5; i += 1) buffer.append(`s-${i}`, 'stdout', 'x');
  expect(buffer.sessionIds()).toHaveLength(MAX_RETAINED_SESSIONS);
  expect(buffer.has('s-0')).toBe(false);
  expect(buffer.has(`s-${MAX_RETAINED_SESSIONS + 4}`)).toBe(true);
});

describe('session output store', () => {
  beforeEach(() => useSessionOutputStore.getState().clear());

  test('appends bump only that session revision and stop events apply the policy', () => {
    const store = useSessionOutputStore.getState();
    store.append('a', 'stdout', 'hello');
    store.append('b', 'stdout', 'hi');
    store.append('a', 'stderr', 'oops');
    expect(useSessionOutputStore.getState().revisions).toEqual({ a: 2, b: 1 });
    expect(useSessionOutputStore.getState().buffer.text('a')).toEqual({ stdout: 'hello\n', stderr: 'oops\n', truncated: false });

    store.sessionChanged(session('a', 'stopped'));
    for (let i = 0; i < MAX_FINISHED_SESSIONS; i += 1) {
      store.append(`done-${i}`, 'stdout', 'x');
      store.sessionChanged(session(`done-${i}`, 'error'));
    }
    const { revisions, buffer } = useSessionOutputStore.getState();
    expect(buffer.has('a')).toBe(false);
    expect(revisions.a).toBeUndefined();
    expect(revisions.b).toBe(1);
  });

  test('a welcome list keeps live sessions and finishes the rest', () => {
    const store = useSessionOutputStore.getState();
    store.append('live', 'stdout', 'x');
    store.append('old', 'stdout', 'x');
    store.sessionsReplaced([session('live', 'running')]);
    for (let i = 0; i < MAX_FINISHED_SESSIONS; i += 1) {
      store.append(`done-${i}`, 'stdout', 'x');
      store.sessionChanged(session(`done-${i}`, 'stopped'));
    }
    expect(Object.keys(useSessionOutputStore.getState().revisions)).toContain('live');
    expect(useSessionOutputStore.getState().revisions.old).toBeUndefined();
  });
});
