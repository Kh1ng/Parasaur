import { lstatSync, mkdirSync, readdirSync, rmSync, symlinkSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

// Run only after npm ci in the staged runtime. Each link stays relative to the
// server output, so moving the complete dist directory preserves resolution.
const runtime = dirname(fileURLToPath(import.meta.url));
const modules = join(dirname(runtime), 'node_modules');
rmSync(modules, { recursive: true, force: true });
mkdirSync(modules);
for (const source of [join(runtime, 'node_modules'), join(runtime, 'apps/server/node_modules')]) {
  let entries;
  try { entries = readdirSync(source); } catch (error) { if (error.code === 'ENOENT') continue; throw error; }
  for (const entry of entries.filter(name => !name.startsWith('.'))) {
    const names = entry.startsWith('@') ? readdirSync(join(source, entry)).map(name => join(entry, name)) : [entry];
    for (const name of names) {
      const target = join(modules, name);
      mkdirSync(dirname(target), { recursive: true });
      // Workspace-local versions override hoisted versions, matching Node's
      // normal lookup from apps/server. Their own dependencies stay in the
      // complete npm tree, including nested and optional platform packages.
      try { lstatSync(target); rmSync(target, { force: true }); } catch (error) { if (error.code !== 'ENOENT') throw error; }
      symlinkSync(relative(dirname(target), join(source, name)), target, 'dir');
    }
  }
}
