import assert from 'node:assert/strict';
import { test } from 'node:test';
import { chmodSync, existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { preserveCorruptFile, writeFileAtomic } from './atomicFile.js';

const modeOf = (path: string) => (statSync(path).mode & 0o777).toString(8);

test('atomic writes create private files, replace existing ones, and leave no temporary file', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-atomic-'));
  try {
    const path = join(dir, 'nested', 'state.json');
    writeFileAtomic(path, '{"a":1}');
    assert.equal(readFileSync(path, 'utf8'), '{"a":1}');
    if (process.platform !== 'win32') {
      assert.equal(modeOf(path), '600');
      assert.equal(modeOf(join(dir, 'nested')), '700');
    }

    chmodSync(path, 0o644);
    writeFileAtomic(path, '{"a":2}');
    assert.equal(readFileSync(path, 'utf8'), '{"a":2}');
    if (process.platform !== 'win32') assert.equal(modeOf(path), '600', 'a replaced file is private again');
    assert.deepEqual(readdirSync(join(dir, 'nested')), ['state.json']);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a failed atomic write keeps the previous contents and removes its temporary file', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-atomic-'));
  try {
    const path = join(dir, 'state');
    writeFileSync(path, 'old');
    // Renaming a file over a directory fails after the temporary file is written.
    const blocked = join(dir, 'blocked');
    writeFileAtomic(join(blocked, 'placeholder'), 'x');
    assert.throws(() => writeFileAtomic(blocked, 'new'));
    assert.equal(readFileSync(path, 'utf8'), 'old');
    assert.deepEqual(readdirSync(dir).sort(), ['blocked', 'state']);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('a corrupt file is moved aside under a timestamped name', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-atomic-'));
  try {
    const path = join(dir, 'state.json');
    writeFileSync(path, '{torn');
    const preserved = preserveCorruptFile(path, new Date('2026-01-02T03:04:05.678Z'));
    assert.equal(preserved, `${path}.corrupt-2026-01-02T03-04-05-678Z`);
    assert.equal(existsSync(path), false);
    assert.equal(readFileSync(preserved!, 'utf8'), '{torn');
    assert.equal(preserveCorruptFile(path), null, 'a missing file reports that nothing was moved');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
