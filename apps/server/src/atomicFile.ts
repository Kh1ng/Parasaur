/**
 * Replacement and recovery rules for small server state files. A reader sees
 * either the previous contents or the new contents, never a torn write.
 */

import { closeSync, fchmodSync, fsyncSync, mkdirSync, openSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { dirname } from 'node:path';

/**
 * Write `data` to a new temporary file beside `path`, flush it, then rename it
 * over `path`. The file gets `mode` (private by default) whatever the umask is.
 */
export function writeFileAtomic(path: string, data: string, mode = 0o600): void {
  const directory = dirname(path);
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const temporary = `${path}.${process.pid}.${randomBytes(6).toString('hex')}.tmp`;
  try {
    const file = openSync(temporary, 'wx', mode);
    try {
      fchmodSync(file, mode);
      writeFileSync(file, data);
      fsyncSync(file);
    } finally {
      closeSync(file);
    }
    renameSync(temporary, path);
  } catch (error) {
    rmSync(temporary, { force: true });
    throw error;
  }
  syncDirectory(directory);
}

/** Persist the rename itself. Some platforms cannot open a directory; the rename already happened. */
function syncDirectory(directory: string): void {
  let folder: number;
  try {
    folder = openSync(directory, 'r');
  } catch {
    return;
  }
  try {
    fsyncSync(folder);
  } catch {
    // Not supported for directories on this platform.
  } finally {
    closeSync(folder);
  }
}

/**
 * Move an unreadable state file aside for inspection. Returns the new path, or
 * null when the file could not be moved and is still at `path`.
 */
export function preserveCorruptFile(path: string, now = new Date()): string | null {
  const preserved = `${path}.corrupt-${now.toISOString().replace(/[:.]/g, '-')}`;
  try {
    renameSync(path, preserved);
    return preserved;
  } catch {
    return null;
  }
}
