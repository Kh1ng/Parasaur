import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { ActivityFeed, activityFromChat } from './activityFeed.js';

test('chat causes stay separate by session and full error, survive restart, and reset on success', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'gah-chat-transitions-'));
  const path = join(directory, 'activity.jsonl');
  const delivered: string[] = [];
  const deliver = (event: { id: string }) => { delivered.push(event.id); };
  let feed = new ActivityFeed(path, deliver);
  let turn = 0;
  const failure = (error: string, sessionId = 'one') => {
    const event = activityFromChat({ profile: 'gah', sessionId, turn: ++turn, occurredAt: new Date().toISOString(), phase: 'end', outcome: 'error', error });
    assert.ok(event);
    return feed.record(event);
  };
  try {
    assert.equal(failure('ACP connection closed'), true);
    assert.equal(failure('ACP connection closed'), false);
    feed = new ActivityFeed(path, deliver);
    assert.equal(failure('ACP connection closed'), false, 'restart keeps the streak');
    assert.equal(feed.notifications()[0].retryCount, 2);
    assert.equal(failure('ACP connection closed', 'two'), true, 'a different session announces');
    const prefix = 'x'.repeat(210);
    assert.equal(failure(`${prefix}cause A`), true);
    assert.equal(failure(`${prefix}cause B`), true, 'the preview is not the cause identity');
    assert.equal(failure('ACP connection closed'), true, 'returning to a prior cause announces');
    const complete = activityFromChat({ profile: 'gah', sessionId: 'one', turn: ++turn, occurredAt: new Date().toISOString(), phase: 'end', outcome: 'complete' });
    assert.ok(complete);
    feed.record(complete, false);
    assert.equal(failure('ACP connection closed'), true);
    await new Promise(done => setImmediate(done));
    assert.equal(delivered.length, 6);
    assert.equal(new ActivityFeed(path).notifications().find(event => event.id.includes(':one:1:'))?.retryCount, 2);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
