import { accessSync, constants, statSync } from 'node:fs';
import path from 'node:path';

/** Finds a provider CLI the way dispatch does (`resolve_executable_on_path`):
 * the first executable file named `name` on PATH. Copies bundled under a
 * `node_modules` directory are skipped -- npm puts `node_modules/.bin` first
 * on PATH, and those are SDK builds, not the CLI the node has installed.
 * Relative PATH entries are skipped too, so the result never depends on the
 * server's working directory. Returns null when nothing is installed. */
export function findInstalledExecutable(name: string, searchPath = process.env.PATH ?? ''): string | null {
  for (const dir of searchPath.split(path.delimiter)) {
    if (!dir || !path.isAbsolute(dir) || dir.split(path.sep).includes('node_modules')) continue;
    const candidate = path.join(dir, name);
    try {
      if (!statSync(candidate).isFile()) continue;
      accessSync(candidate, constants.X_OK);
      return candidate;
    } catch {
      // Missing or not executable: keep searching, as dispatch does.
    }
  }
  return null;
}
