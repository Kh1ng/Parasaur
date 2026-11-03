import { existsSync, realpathSync, rmSync } from 'node:fs';
import { isAbsolute, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Release bundles embed these packages. Source builds must resolve the newly
// compiled workspaces instead of keeping the previous release's imports.
const server = fileURLToPath(new URL('../apps/server/', import.meta.url));
const target = resolve(server, 'dist/node_modules/@git-agent-harness');
if (existsSync(target)) {
  const location = relative(realpathSync(server), realpathSync(target));
  if (location === '..' || location.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`) || isAbsolute(location)) {
    throw new Error('Refusing to clean a release runtime outside the server directory.');
  }
  rmSync(target, { recursive: true, force: true });
}
