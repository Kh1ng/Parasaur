import { realpathSync } from 'node:fs';
import path from 'node:path';

function isInside(child: string, parent: string): boolean {
  return child === parent || child.startsWith(parent + path.sep);
}

/** getcwd() may hand the backend child a symlink-resolved cwd (macOS
 * /var -> /private/var), and the emitted file_path is absolute, so
 * containment is judged on canonical paths -- falling back to the lexical
 * form only for a target that does not exist on disk yet. A target that
 * realpath-resolves somewhere new (a symlink escape) is judged on its
 * canonical form alone, never its lexical one. */
export function isInsideWorkingDirectory(resolved: string, cwd: string): boolean {
  const canonical = (target: string): string => {
    try {
      return realpathSync(target);
    } catch {
      return target;
    }
  };
  const realCwd = canonical(cwd);
  const realResolved = canonical(resolved);
  if (realResolved !== resolved) return isInside(realResolved, realCwd);
  return isInside(resolved, cwd) || isInside(resolved, realCwd);
}
