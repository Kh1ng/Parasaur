import assert from 'node:assert/strict';
import http from 'node:http';
import { test } from 'node:test';

// Fails with a server still listening, as a failed assertion that skips its
// cleanup does: the runner must exit and still report the failure.
test('fails and leaves a server listening', async () => {
  await new Promise<void>((done) => http.createServer().listen(0, '127.0.0.1', done));
  assert.equal(1, 2, 'deliberate failure');
});
