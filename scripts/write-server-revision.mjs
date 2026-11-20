// Tie the server's update comparison to the compiled deployment. A checkout
// can move after a build, and a worker kit can be installed independently.
import { spawnSync } from 'node:child_process';
import { existsSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const git = (...args) => spawnSync('git', args, { cwd: root, encoding: 'utf8' });
let revision = null;
let ancestors = [];
if (existsSync(new URL('../.git', import.meta.url))) {
  const status = git('status', '--porcelain', '--untracked-files=normal', '--',
    'apps/server/src', 'apps/server/package.json', 'apps/server/tsconfig.json', 'packages/contracts',
    'packages/shared', 'scripts/write-server-revision.mjs', 'tsconfig.base.json', 'package.json', 'package-lock.json'
  );
  const head = git('rev-parse', 'HEAD');
  const sha = head.stdout?.trim();
  if (status.status === 0 && !status.stdout.trim() && head.status === 0 && /^[0-9a-f]{40}$/.test(sha)) {
    revision = sha;
    // Release nodes can compare builds offline. A shallow developer checkout
    // proves ancestry only for the commits it actually contains.
    const history = git('rev-list', '--max-count=4096', sha);
    if (history.status === 0) ancestors = history.stdout.trim().split(/\s+/).filter(value => value !== sha);
  }
}
writeFileSync(new URL('../apps/server/dist/build-revision.json', import.meta.url), JSON.stringify({ revision, ancestors }) + '\n');
