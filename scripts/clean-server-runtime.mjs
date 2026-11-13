import { existsSync, readFileSync, realpathSync, rmSync } from 'node:fs';
import { isAbsolute, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Release bundles embed these packages. Source builds must resolve the newly
// compiled workspaces instead of keeping the previous release's imports.
const server = fileURLToPath(new URL('../apps/server/', import.meta.url));
const runtime = resolve(server, 'dist/.release-runtime');
const managed = existsSync(runtime);
const targets = managed
  ? [resolve(server, 'dist/node_modules'), runtime]
  : [resolve(server, 'dist/node_modules/@git-agent-harness')];
// Check every target before removing any of them.
for (const target of targets.filter(existsSync)) {
  const location = relative(realpathSync(server), realpathSync(target));
  if (location === '..' || location.startsWith(`..${process.platform === 'win32' ? '\\' : '/'}`) || isAbsolute(location)) {
    throw new Error('Refusing to clean a release runtime outside the server directory.');
  }
}
if (managed && JSON.parse(readFileSync(resolve(runtime, 'format.json'), 'utf8')).schema !== 1) {
  throw new Error('Unsupported release runtime cleanup schema');
}
for (const target of targets) rmSync(target, { recursive: true, force: true });
