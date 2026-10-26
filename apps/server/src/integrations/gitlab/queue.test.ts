import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { classifyEvent } from './classifier.js';
import { DISPATCH_RETRY_MS, GitLabEventStore, MAX_DISPATCH_ATTEMPTS, type GitLabDelivery } from './eventStore.js';
import { GitLabEventQueue } from './queue.js';

function delivery(name = 'issue', eventUuid?: string): GitLabDelivery {
  const body: unknown = JSON.parse(readFileSync(new URL(`../../../tests/fixtures/gitlab-19/${name}.json`, import.meta.url), 'utf8'));
  const event = classifyEvent(body, 'parasaur', true, eventUuid);
  assert.ok(event);
  return { key: event.key, body, eventUuid, bot: 'parasaur', pipelineEnabled: true, panelUrl: 'https://panel.example.com/' };
}

test('queue is private, bounded, pins pending receipts past TTL, and migrates receipt-only files', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gitlab-queue-'));
  try {
    const path = join(dir, 'events.json');
    let now = 0;
    const first = delivery();
    const legacy = delivery('note');
    writeFileSync(path, JSON.stringify([[legacy.key, 10]]));
    let store = new GitLabEventStore(10, 10, () => now, path, 1);
    assert.equal(store.enqueue(legacy), 'duplicate');
    assert.equal(store.enqueue(first), 'new');
    assert.equal(statSync(path).mode & 0o777, 0o600);
    assert.equal(JSON.parse(readFileSync(path, 'utf8')).version, 1);
    now = 100;
    store = new GitLabEventStore(10, 10, () => now, path, 1);
    assert.equal(store.enqueue(first), 'duplicate', 'pending work does not expire');
    assert.equal(store.enqueue(legacy), 'queue_full');
    store.complete(first.key);
    assert.equal(store.enqueue(first), 'duplicate', 'receipt TTL starts again at completion');
    assert.equal(store.enqueue(legacy), 'new');
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('poison attempts survive restarts, do not block healthy work, and log the dead-letter transition once', async t => {
  const log = t.mock.method(console, 'error', () => {});
  const dir = mkdtempSync(join(tmpdir(), 'gitlab-poison-'));
  try {
    const path = join(dir, 'events.json');
    let now = 0;
    const poison = delivery();
    const healthy = delivery('note');
    const fresh = () => new GitLabEventStore(10, 10, () => now, path, 2);
    const initial = fresh();
    initial.enqueue(poison);
    initial.enqueue(healthy);
    const calls: string[] = [];
    for (let attempt = 1; attempt <= MAX_DISPATCH_ATTEMPTS; attempt++) {
      const worker = new GitLabEventQueue(fresh(), async event => {
        calls.push(event.key);
        if (event.key === poison.key) throw new Error('sensitive provider failure');
      });
      await Promise.all([worker.drain(), worker.drain()]);
      await worker.drain();
      const saved = JSON.parse(readFileSync(path, 'utf8'));
      assert.equal((saved.pending[0] ?? saved.deadLetters[0]).attempts, attempt);
      assert.equal(calls.filter(key => key === poison.key).length, attempt, 'retry delay and concurrent drains do not repeat work');
      assert.equal(calls.filter(key => key === healthy.key).length, 1, 'healthy work passes the failing entry');
      now += DISPATCH_RETRY_MS;
    }
    const state = JSON.parse(readFileSync(path, 'utf8'));
    assert.equal(state.pending.length, 0);
    assert.equal(state.deadLetters.length, 1);
    assert.equal(state.deadLetters[0].key, poison.key);
    assert.equal(log.mock.callCount(), 1);
    assert.match(String(log.mock.calls[0].arguments[0]), /moved to dead letters after 3 dispatch failures/);
    assert.ok(!readFileSync(path, 'utf8').includes('sensitive provider failure'));
    await new GitLabEventQueue(fresh(), async () => { assert.fail('dead letters must not replay'); }).drain();
    assert.equal(log.mock.callCount(), 1, 'restart does not announce the same transition');
    const store = fresh();
    assert.equal(store.enqueue(poison), 'duplicate');
    assert.equal(store.enqueue(delivery('merge_request')), 'new');
    assert.equal(store.enqueue(delivery('pipeline')), 'queue_full', 'dead letters count against the bound and are never evicted');
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('pending payload remains on disk until dispatch records its result', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'gitlab-pending-dispatch-'));
  let release = () => {};
  const recorded = new Promise<void>(resolve => { release = resolve; });
  const path = join(dir, 'events.json');
  const store = new GitLabEventStore(10, 100, Date.now, path);
  const accepted = delivery();
  store.enqueue(accepted);
  const queue = new GitLabEventQueue(store, async () => { await recorded; });
  const drain = queue.drain();
  try {
    assert.equal(JSON.parse(readFileSync(path, 'utf8')).pending.length, 1);
    assert.equal(new GitLabEventStore(10, 100, Date.now, path).enqueue(accepted), 'duplicate');
    release();
    await drain;
    assert.equal(JSON.parse(readFileSync(path, 'utf8')).pending.length, 0);
  } finally {
    release(); await drain;
    rmSync(dir, { recursive: true, force: true });
  }
});

test('an oversized serialized payload is refused before acceptance and cannot poison restart', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gitlab-payload-bound-'));
  try {
    const path = join(dir, 'events.json');
    const store = new GitLabEventStore(10, 100, Date.now, path);
    const accepted = delivery();
    const body = { ...accepted.body as object, extra: Array(7_000).fill(1e20) };
    // Small exponent notation can expand past the input limit when JSON is saved.
    assert.throws(() => store.enqueue({ ...accepted, body }));
    assert.equal(store.enqueue(accepted), 'new');
    assert.equal(new GitLabEventStore(10, 100, Date.now, path).ready().length, 1);
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('failed atomic writes neither reserve new work nor remove accepted work', async t => {
  const log = t.mock.method(console, 'error', () => {});
  const dir = mkdtempSync(join(tmpdir(), 'gitlab-write-failure-'));
  try {
    const path = join(dir, 'events.json');
    const backup = join(dir, 'backup.json');
    const store = new GitLabEventStore(10, 100, Date.now, path);
    const first = delivery();
    store.enqueue(first);
    renameSync(path, backup);
    mkdirSync(path);
    assert.throws(() => store.enqueue(delivery('note')));
    assert.throws(() => store.complete(first.key));
    assert.equal(store.ready().length, 1);
    const queue = new GitLabEventQueue(store, async () => {});
    await queue.drain();
    await queue.drain();
    assert.equal(log.mock.callCount(), 1, 'repeated persistence errors stay quiet');
    assert.equal(JSON.parse(readFileSync(backup, 'utf8')).pending.length, 1);
    rmSync(path, { recursive: true });
    renameSync(backup, path);
    assert.equal(store.enqueue(delivery('note')), 'new', 'failed acceptance did not reserve a receipt');
    await queue.drain();
    assert.equal(JSON.parse(readFileSync(path, 'utf8')).pending.length, 0);
    store.enqueue(delivery('merge_request'));
    renameSync(path, backup);
    mkdirSync(path);
    await queue.drain();
    assert.equal(log.mock.callCount(), 2, 'recovery resets the failure transition');
  } finally { rmSync(dir, { recursive: true, force: true }); }
});

test('invalid queue state fails closed and keeps the original file', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gitlab-invalid-queue-'));
  try {
    const path = join(dir, 'events.json');
    const accepted = delivery();
    const entry = { ...accepted, attempts: 0, retryAt: 0 };
    const valid = { version: 1, receipts: [[accepted.key, 100]], pending: [entry], deadLetters: [] };
    for (const corrupt of [
      { ...valid, version: 2 },
      { ...valid, receipts: [] },
      { ...valid, pending: [entry, entry] },
      { ...valid, pending: [{ ...entry, key: 'b'.repeat(64) }] },
      { ...valid, pending: [{ ...entry, panelUrl: 'https://user:password@panel.example.com/' }] },
      { ...valid, pending: [{ ...entry, attempts: MAX_DISPATCH_ATTEMPTS }] },
      { ...valid, pending: [], deadLetters: [entry] },
      { ...valid, pending: [{ ...entry, body: { object_kind: 'push' } }] },
    ]) {
      const contents = JSON.stringify(corrupt);
      writeFileSync(path, contents);
      assert.throws(() => new GitLabEventStore(10, 100, Date.now, path).enqueue(accepted));
      assert.equal(readFileSync(path, 'utf8'), contents);
    }
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
