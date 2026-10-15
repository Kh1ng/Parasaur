import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { autoApprovedOption, type PermissionAsk } from './permissionPolicy.js';
import { readSettings } from './settingsStore.js';

const options = [
  { optionId: 'allow-once', name: 'Allow', kind: 'allow_once' },
  { optionId: 'allow-always', name: 'Always', kind: 'allow_always' },
  { optionId: 'reject-once', name: 'Reject', kind: 'reject_once' }
];

function withWorktree(run: (worktree: string, outside: string) => void) {
  const root = mkdtempSync(join(tmpdir(), 'gah-permission-policy-'));
  const worktree = join(root, 'worktree');
  mkdirSync(join(worktree, 'src'), { recursive: true });
  writeFileSync(join(worktree, 'src', 'main.ts'), '');
  try {
    run(worktree, root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

const ask = (fields: Partial<PermissionAsk>): PermissionAsk => ({ title: 'action', options, locations: [], ...fields });

test('ask mode and an unknown mode never auto-approve', () => withWorktree((worktree) => {
  const edit = ask({ kind: 'edit', locations: [join(worktree, 'src/main.ts')] });
  assert.equal(autoApprovedOption('ask', edit, worktree), null);
  assert.equal(autoApprovedOption('Bypass', edit, worktree), null);
  assert.equal(autoApprovedOption('bypass', edit, worktree), 'allow-once', 'only allow_once is chosen, never allow_always');
}));

test('bypass approves ordinary in-worktree reads, edits, and local commands', () => withWorktree((worktree) => {
  assert.equal(autoApprovedOption('bypass', ask({ kind: 'read', locations: ['src/main.ts'] }), worktree), 'allow-once');
  assert.equal(autoApprovedOption('bypass', ask({ kind: 'edit', locations: [join(worktree, 'src/new.ts')] }), worktree), 'allow-once');
  for (const command of ['cargo test', 'npm run build', 'git commit -m "fix: a thing"', 'ls src/', ['bash', '-lc', 'git status']]) {
    assert.equal(autoApprovedOption('bypass', ask({ kind: 'execute', rawInput: { command } }), worktree), 'allow-once', String(command));
  }
}));

test('bypass still asks for a push or other publish', () => withWorktree((worktree) => {
  for (const command of [
    'git push origin HEAD', ['git', 'push'], 'git -C . push', 'gh pr create', 'npm publish', 'cargo publish',
    'docker push image', 'git remote add x y', 'curl -X POST https://example.test', 'ssh host', 'env git push',
    'git status && git push', 'git status; git push', 'echo $(git push)', ['bash', '-c', 'git push'], ['sh', '-c', 'a', 'b']
  ]) {
    assert.equal(autoApprovedOption('bypass', ask({ kind: 'execute', rawInput: { command } }), worktree), null, String(command));
  }
}));

test('bypass still asks for anything outside the worktree', () => withWorktree((worktree, outside) => {
  symlinkSync(outside, join(worktree, 'escape'));
  const cases: Partial<PermissionAsk>[] = [
    { kind: 'edit', locations: ['/etc/hosts'] },
    { kind: 'read', locations: [join(worktree, 'src/main.ts'), '/tmp/other'] },
    { kind: 'edit', locations: ['../sibling/file'] },
    { kind: 'edit', locations: [join(worktree, 'escape', 'new-file')] },
    { kind: 'edit', locations: ['src/main.ts'], rawInput: { file_path: '/etc/passwd' } },
    { kind: 'execute', rawInput: { command: 'cat /etc/passwd' } },
    { kind: 'execute', rawInput: { command: 'cp ../secret src/' } },
    { kind: 'execute', rawInput: { command: 'npm test', cwd: '/tmp' } },
    { kind: 'execute', rawInput: { command: 'rm --file=/tmp/x' } },
    { kind: 'execute', rawInput: { command: 'cat ~/.ssh/id_rsa' } }
  ];
  for (const fields of cases) assert.equal(autoApprovedOption('bypass', ask(fields), worktree), null, JSON.stringify(fields));
}));

// PR #1493 review: relative arguments are paths too, and `..` after a
// symlink climbs from the link's physical target, not from the link name.
test('bypass judges every path on the filesystem, not lexically', () => withWorktree((worktree, outside) => {
  mkdirSync(join(outside, 'nested'));
  writeFileSync(join(outside, 'secret.txt'), 'secret');
  symlinkSync(outside, join(worktree, 'escape'));
  symlinkSync(join(outside, 'nested'), join(worktree, 'link'));
  const cases: Partial<PermissionAsk>[] = [
    { kind: 'execute', rawInput: { command: 'cat escape/secret.txt' } },
    { kind: 'execute', rawInput: { command: ['cat', 'escape/secret.txt'] } },
    { kind: 'execute', rawInput: { command: 'cat link/../secret.txt' } },
    { kind: 'execute', rawInput: { command: 'cp --target-directory=escape src/main.ts' } },
    { kind: 'read', locations: ['link/../secret.txt'] },
    { kind: 'edit', locations: [`${worktree}/link/../secret.txt`] },
    { kind: 'edit', locations: ['src/main.ts'], rawInput: { file_path: 'link/../secret.txt' } },
    { kind: 'edit', locations: ['missing/../../secret.txt'] }
  ];
  const approved = cases.filter((fields) => autoApprovedOption('bypass', ask(fields), worktree) !== null);
  assert.deepEqual(approved, [], 'every case reaches outside the worktree');
  assert.equal(autoApprovedOption('bypass', ask({ kind: 'read', locations: ['src/../src/main.ts'] }), worktree), 'allow-once', 'a real in-worktree `..` still approves');
}));

test('bypass asks for requests it cannot classify', () => withWorktree((worktree) => {
  const cases: Partial<PermissionAsk>[] = [
    { kind: null, locations: ['src/main.ts'] },
    { kind: 'other', locations: ['src/main.ts'] },
    { kind: 'fetch' },
    { kind: 'think' },
    { kind: 'edit', locations: [] },
    { kind: 'execute' },
    { kind: 'execute', rawInput: { command: 42 } },
    { kind: 'execute', rawInput: { command: 'echo "unterminated' } },
    { kind: 'read', locations: ['src/main.ts'], options: [{ optionId: 'x', name: 'Always', kind: 'allow_always' }] }
  ];
  for (const fields of cases) assert.equal(autoApprovedOption('bypass', ask(fields), worktree), null, JSON.stringify(fields));
  assert.equal(autoApprovedOption('bypass', ask({ kind: 'read', locations: ['src/main.ts'] }), undefined), null, 'no worktree (default chat) asks');
}));

test('a stored permission mode other than bypass reads as ask', () => {
  const dir = mkdtempSync(join(tmpdir(), 'gah-permission-mode-'));
  const previous = process.env.GAH_MANAGER_CHAT_SETTINGS_PATH;
  process.env.GAH_MANAGER_CHAT_SETTINGS_PATH = join(dir, 'settings.json');
  try {
    assert.equal(readSettings().permissionMode, 'ask', 'missing file');
    for (const stored of [undefined, 'BYPASS', 'allow', true, 1, null]) {
      writeFileSync(process.env.GAH_MANAGER_CHAT_SETTINGS_PATH, JSON.stringify({ defaultBackend: 'hermes', permissionMode: stored }));
      assert.equal(readSettings().permissionMode, 'ask', String(stored));
    }
    writeFileSync(process.env.GAH_MANAGER_CHAT_SETTINGS_PATH, JSON.stringify({ permissionMode: 'bypass' }));
    assert.equal(readSettings().permissionMode, 'bypass');
  } finally {
    if (previous === undefined) delete process.env.GAH_MANAGER_CHAT_SETTINGS_PATH;
    else process.env.GAH_MANAGER_CHAT_SETTINGS_PATH = previous;
    rmSync(dir, { recursive: true, force: true });
  }
});
