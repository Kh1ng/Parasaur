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
 * With required directory sync, a post-rename error has an uncertain commit
 * outcome: callers must not assume that the previous contents remain.
 */
export function writeFileAtomic(path: string, data: string, mode = 0o600, requireDirectorySync = false): void {
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
  syncDirectory(directory, requireDirectorySync);
}

/**
 * Write a private file that is handed to one child process and then deleted.
 * It is new (never an existing file or link), private whatever the umask is,
 * and complete when this returns. Nothing is flushed to disk: a crash loses
 * only a file nobody will read again, while each flush stops the whole server
 * for as long as the disk takes to commit.
 */
export function writeScratchFile(path: string, data: string, mode = 0o600): void {
  const file = openSync(path, 'wx', mode);
  let complete = false;
  try {
    fchmodSync(file, mode);
    writeFileSync(file, data);
    complete = true;
  } finally {
    closeSync(file);
    // A partial file must not be mistaken for the whole one.
    if (!complete) try { rmSync(path, { force: true }); } catch { /* The write error is the one to report. */ }
  }
}

/** Persist the rename itself. Some platforms cannot open a directory; the rename already happened. */
function syncDirectory(directory: string, required: boolean): void {
  let folder: number;
  try {
    folder = openSync(directory, 'r');
  } catch (error) {
    if (required) throw error;
    return;
  }
  try {
    fsyncSync(folder);
  } catch (error) {
    if (required) throw error;
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
