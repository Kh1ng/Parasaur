// Test support: holds the chat journal write of a permission request so a
// test can act (cancel, reconnect) while the write is in flight.
import assert from 'node:assert/strict';
import { WriteStream } from 'node:fs';
import type { TestContext } from 'node:test';

/** Hold the actual journal write, including batches, while websocket IO runs. */
export function delayPermissionWrite(t: TestContext, failure?: Error): Promise<void> {
  let writing!: () => void;
  const started = new Promise<void>((resolve) => { writing = resolve; });
  let held = false;
  t.after(() => assert.ok(held, 'the test intercepted a permission journal write'));
  const write = WriteStream.prototype._write;
  const writev = WriteStream.prototype._writev;
  assert.ok(writev);
  t.after(() => { WriteStream.prototype._writev = writev; });
  t.mock.method(WriteStream.prototype, '_write', function(this: WriteStream, chunk: Buffer, encoding: BufferEncoding, callback: (error?: Error | null) => void) {
    if (!chunk.toString().includes('"type":"permission/request"')) return write.call(this, chunk, encoding, callback);
    held = true;
    writing();
    setTimeout(() => failure ? callback(failure) : write.call(this, chunk, encoding, callback), 250);
  });
  WriteStream.prototype._writev = function(chunks, callback) {
    if (!chunks.some(({ chunk }) => chunk.toString().includes('"type":"permission/request"'))) return writev.call(this, chunks, callback);
    held = true;
    writing();
    setTimeout(() => failure ? callback(failure) : writev.call(this, chunks, callback), 250);
  };
  return started;
}
