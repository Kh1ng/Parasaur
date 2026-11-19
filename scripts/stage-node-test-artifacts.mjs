// Package existing build outputs with their source revision and content hashes.
import { copyFileSync, mkdirSync, mkdtempSync, renameSync, rmdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { join, resolve } from 'node:path';

const [kind, destination, builtCli] = process.argv.slice(2);
const installWorker = kind === 'worker' && builtCli !== undefined;
if (!['desktop', 'worker'].includes(kind) || !destination) throw new Error('Usage: node scripts/stage-node-test-artifacts.mjs desktop|worker DESTINATION');
const revision = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
const final = resolve(destination);
mkdirSync(final, { recursive: true });
// The manifest is replaced last. Windows verifies all bytes against one
// manifest before changing WSL settings, including during a central update.
const output = installWorker ? mkdtempSync(join(final, '.stage-')) : final;
if (readdirSync(output).length) throw new Error('Artifact destination must be empty.');
const sources = { 'install-windows.ps1': 'scripts/install-windows.ps1' };
if (kind === 'desktop') {
  const directory = 'apps/desktop/target/release/bundle/nsis';
  const installers = readdirSync(directory).filter(name => /^[\w .-]+_\d+\.\d+\.\d+_x64-setup\.exe$/.test(name));
  if (installers.length !== 1) throw new Error('Expected exactly one Windows x64 NSIS installer.');
  sources[installers[0]] = join(directory, installers[0]);
} else {
  sources.gah = builtCli || 'target/release/gah';
  sources['install-wsl.sh'] = 'scripts/install-wsl-worker.sh';
  execFileSync('git', ['archive', '--format=tar.gz', `--output=${join(output, 'source.tar.gz')}`, revision]);
}
for (const [name, source] of Object.entries(sources)) {
  // Export scripts from Git so Windows checkout CRLF conversion cannot mix bundle hashes.
  if (source.startsWith('scripts/')) writeFileSync(join(output, name), execFileSync('git', ['show', `${revision}:${source}`]));
  else copyFileSync(source, join(output, name));
}
const names = [...Object.keys(sources), ...(kind === 'worker' ? ['source.tar.gz'] : [])];
const files = Object.fromEntries(names.map(name => [name, createHash('sha256').update(readFileSync(join(output, name))).digest('hex')]));
writeFileSync(join(output, `${kind}-artifact.json`), JSON.stringify({ schema_version: 1, revision, files }, null, 2) + '\n');

if (installWorker) {
  for (const name of [...names, 'worker-artifact.json']) renameSync(join(output, name), join(final, name));
  rmdirSync(output);
}
