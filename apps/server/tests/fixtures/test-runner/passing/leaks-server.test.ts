import http from 'node:http';
import { test } from 'node:test';

// Leaves a server listening on purpose: this file's process only exits when
// the runner forces it to (see src/testRunner.test.ts).
test('passes and leaves a server listening', async () => {
  await new Promise<void>((done) => http.createServer().listen(0, '127.0.0.1', done));
});
