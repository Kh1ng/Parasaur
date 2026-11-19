// Package existing build outputs with their source revision and content hashes.
import { copyFileSync, createReadStream, mkdirSync, mkdtempSync, renameSync, rmdirSync, rmSync, readdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';

const [kind, destination, builtCli, runtimeOption] = process.argv.slice(2);
if (runtimeOption !== undefined && (kind !== 'worker' || runtimeOption !== '--bundle-runtime')) throw new Error('Expected --bundle-runtime with a worker CLI');
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
  const archive = join(output, 'source.tar.gz');
  execFileSync('git', ['archive', '--format=tar.gz', `--output=${archive}`, revision]);
  if (runtimeOption) {
    const payload = mkdtempSync(join(tmpdir(), 'parasaur-worker-runtime-'));
    try {
      execFileSync('tar', ['-xzf', archive, '-C', payload]);
      // Worker ACP sessions can use their adapters' bundled Linux executables.
      execFileSync('bash', ['scripts/stage-linux-server-runtime.sh', payload, '--include=optional'], { stdio: 'inherit' });
      execFileSync('tar', ['-czf', archive, '-C', payload, '.']);
    } finally {
      rmSync(payload, { recursive: true, force: true });
    }
  }
}
for (const [name, source] of Object.entries(sources)) {
  // Export scripts from Git so Windows checkout CRLF conversion cannot mix bundle hashes.
  if (source.startsWith('scripts/')) writeFileSync(join(output, name), execFileSync('git', ['show', `${revision}:${source}`]));
  else copyFileSync(source, join(output, name));
}
const names = [...Object.keys(sources), ...(kind === 'worker' ? ['source.tar.gz'] : [])];
const files = {};
for (const name of names) {
  const hash = createHash('sha256');
  for await (const bytes of createReadStream(join(output, name))) hash.update(bytes);
  files[name] = hash.digest('hex');
}
writeFileSync(join(output, `${kind}-artifact.json`), JSON.stringify({ schema_version: 1, revision, files }, null, 2) + '\n');

if (installWorker) {
  for (const name of [...names, 'worker-artifact.json']) renameSync(join(output, name), join(final, name));
  rmdirSync(output);
}
