import { cpSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { isAbsolute, relative, resolve } from 'node:path';

const [source, destination] = process.argv.slice(2);
if (!source || !destination) throw new Error('Expected source checkout and runtime staging directory');
const root = realpathSync(source);
const lock = JSON.parse(readFileSync(resolve(root, 'package-lock.json'), 'utf8'));
if (!lock.packages?.['']) throw new Error('Release runtime requires a workspace package-lock');
mkdirSync(destination, { recursive: true });
for (const name of ['package.json', 'package-lock.json']) cpSync(resolve(root, name), resolve(destination, name));
for (const workspace of Object.keys(lock.packages).filter(name => name && !name.split('/').includes('node_modules'))) {
  if (isAbsolute(workspace) || workspace.includes('\\') || workspace.split('/').some(part => !part || part === '.' || part === '..')) {
    throw new Error('Invalid workspace path in release lockfile');
  }
  const manifest = realpathSync(resolve(root, workspace, 'package.json'));
  const path = relative(root, manifest);
  if (isAbsolute(path) || path === '..' || path.startsWith('../') || path.startsWith('..\\')) {
    throw new Error('Workspace manifest escapes the release checkout');
  }
  mkdirSync(resolve(destination, workspace), { recursive: true });
  cpSync(manifest, resolve(destination, workspace, 'package.json'));
}
for (const name of ['contracts', 'shared']) {
  cpSync(resolve(root, 'packages', name, 'dist'), resolve(destination, 'packages', name, 'dist'), { recursive: true });
}
cpSync(new URL('./link-release-runtime.mjs', import.meta.url), resolve(destination, 'link.mjs'));
writeFileSync(resolve(destination, 'format.json'), JSON.stringify({ schema: 1 }));
