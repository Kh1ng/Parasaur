import { lstatSync, readlinkSync } from 'node:fs';
import path from 'node:path';

function isInside(child: string, parent: string): boolean {
  return child === parent || child.startsWith(parent + path.sep);
}

const MAX_SYMLINK_HOPS = 40;

/** Where `target` physically lands, walked one component at a time from
 * the physical directory `from` the way the kernel does: a symlink is
 * followed before the next component, so `link/..` climbs from the link's
 * target, not from the link's own directory. Past the first missing
 * component the rest is new, so it is appended as-is; a `..` there, a
 * symlink loop, or an unreadable component returns null. POSIX paths only. */
function physicalPath(target: string, from: string, hops = { count: 0 }): string | null {
  let current = path.isAbsolute(target) ? path.sep : from;
  let missing = false;
  for (const part of target.split(path.sep)) {
    if (part === '' || part === '.') continue;
    if (part === '..') {
      if (missing) return null;
      current = path.dirname(current);
      continue;
    }
    const next = path.join(current, part);
    if (!missing) {
      let stat;
      try {
        stat = lstatSync(next);
      } catch (error) {
        const code = (error as NodeJS.ErrnoException).code;
        if (code !== 'ENOENT' && code !== 'ENOTDIR') return null;
      }
      if (!stat) {
        missing = true;
      } else if (stat.isSymbolicLink()) {
        if (++hops.count > MAX_SYMLINK_HOPS) return null;
        const linked = physicalPath(readlinkSync(next), current, hops);
        if (linked === null) return null;
        current = linked;
        continue;
      }
    }
    current = next;
  }
  return current;
}

/** Whether `target` (absolute, or relative to `cwd`) physically lands inside
 * `cwd`. Both are walked on the filesystem, so a symlinked cwd (macOS
 * /var -> /private/var) still matches, and neither a symlink nor a `..`
 * after one can escape. Anything that cannot be walked is outside. */
export function isInsideWorkingDirectory(target: string, cwd: string): boolean {
  const realCwd = physicalPath(cwd, path.sep);
  const realTarget = realCwd === null ? null : physicalPath(target, realCwd);
  return realCwd !== null && realTarget !== null && isInside(realTarget, realCwd);
}

/** One backend permission request, as the adapters hand it to the manager. */
export interface PermissionAsk {
  title: string;
  options: { optionId: string; name: string; kind: string }[];
  locations: string[];
  /** ACP ToolKind ('read', 'edit', 'execute', ...) when the backend sent one. */
  kind?: string | null;
  /** The backend's raw tool input (ACP rawInput). Classified, never logged. */
  rawInput?: unknown;
}

/** Kinds whose effect is confined to the paths in `locations`. */
const PATH_KINDS = new Set(['read', 'edit', 'delete', 'move', 'search']);
/** rawInput keys that name a file or directory the tool acts on. */
const PATH_KEYS = ['path', 'file_path', 'filePath', 'notebook_path', 'cwd', 'destination', 'target', 'old_path', 'new_path'];
/** git subcommands that only change the local repository. An alias cannot
 * shadow a built-in name, so this allowlist also stops a `git p` alias. */
const LOCAL_GIT = new Set([
  'status', 'diff', 'log', 'show', 'add', 'commit', 'restore', 'checkout', 'switch', 'branch', 'stash',
  'rev-parse', 'ls-files', 'blame', 'grep', 'merge-base', 'describe', 'reflog', 'mv', 'rm', 'reset',
  'merge', 'rebase', 'cherry-pick', 'fetch'
]);
/** Programs that wrap another command or reach a remote host directly. */
const NOT_LOCAL = new Set([
  'env', 'sudo', 'su', 'doas', 'sh', 'bash', 'zsh', 'fish', 'dash', 'ksh', 'eval', 'exec', 'xargs', 'nohup',
  'time', 'timeout', 'nice', 'command', 'builtin', 'watch', 'npx', 'ssh', 'scp', 'sftp', 'rsync', 'curl',
  'wget', 'nc', 'ncat', 'telnet', 'ftp', 'gh', 'glab', 'hub', 'docker', 'podman', 'kubectl', 'helm', 'aws',
  'gcloud', 'az', 'twine', 'terraform', 'vercel', 'netlify', 'flyctl', 'heroku', 'firebase', 'wrangler'
]);
/** Argument words that publish: `npm publish`, `cargo publish`, `docker push`. */
const PUBLISH_WORDS = new Set(['push', 'publish', 'deploy', 'release', 'upload']);
const SHELLS = new Set(['sh', 'bash', 'zsh']);

/**
 * The option the 'bypass' permission mode picks without asking, or null to
 * ask the human. Null unless the request is an ordinary action inside the
 * session worktree: every location and path argument resolves inside it,
 * the tool kind is known, and a command neither publishes nor wraps another
 * command. Anything this cannot classify asks (issue #1382).
 */
export function autoApprovedOption(mode: string, request: PermissionAsk, worktree: string | undefined): string | null {
  if (mode !== 'bypass' || !worktree || !path.isAbsolute(worktree)) return null;
  const allow = request.options.find((option) => option.kind === 'allow_once');
  if (!allow) return null;
  const inside = (target: string) => isInsideWorkingDirectory(target, worktree);
  if (!request.locations.every(inside)) return null;
  const input = request.rawInput && typeof request.rawInput === 'object' ? request.rawInput as Record<string, unknown> : {};
  for (const key of PATH_KEYS) {
    const value = input[key];
    if (value !== undefined && (typeof value !== 'string' || !inside(value))) return null;
  }
  if (request.kind && PATH_KINDS.has(request.kind)) return request.locations.length > 0 ? allow.optionId : null;
  if (request.kind === 'execute') {
    const argv = commandArgv(input.command);
    return argv && localCommand(argv, inside) ? allow.optionId : null;
  }
  return null;
}

/** argv for a command given as a string or an argv array; an argv of
 * `sh -c <script>` is unwrapped once. Null when it cannot be split safely. */
function commandArgv(command: unknown): string[] | null {
  if (typeof command === 'string') return splitCommand(command);
  if (!Array.isArray(command) || command.length === 0 || !command.every((part) => typeof part === 'string')) return null;
  const [program, flag, script, ...rest] = command as string[];
  if (SHELLS.has(path.basename(program)) && (flag === '-c' || flag === '-lc') && script !== undefined && rest.length === 0) {
    return splitCommand(script);
  }
  return command as string[];
}

/** Splits one simple command on blanks, with plain single or double quotes.
 * Null for anything a shell would treat specially (chaining, pipes,
 * redirection, substitution, globbing, escapes). */
function splitCommand(script: string): string[] | null {
  const argv: string[] = [];
  let current: string | null = null;
  let quote: string | null = null;
  for (const char of script) {
    if (quote) {
      if (char === quote) quote = null;
      else if (quote === '"' && /[$`\\!]/.test(char)) return null;
      else current += char;
    } else if (char === "'" || char === '"') {
      quote = char;
      current ??= '';
    } else if (char === ' ' || char === '\t') {
      if (current !== null) argv.push(current);
      current = null;
    } else if (/[;&|<>()$`\\*?[\]{}~!#\n\r]/.test(char)) {
      return null;
    } else {
      current = (current ?? '') + char;
    }
  }
  if (quote) return null;
  if (current !== null) argv.push(current);
  return argv.length > 0 ? argv : null;
}

function localCommand(argv: string[], inside: (target: string) => boolean): boolean {
  const program = path.basename(argv[0]);
  if (NOT_LOCAL.has(program) || argv.some((arg) => PUBLISH_WORDS.has(arg))) return false;
  if (program === 'git' && !LOCAL_GIT.has(argv[1] ?? '')) return false;
  // Any word may name a file (`cat escape/x`, `--dir=link/..`), so each
  // one, and the value after an `=`, must land inside the worktree when
  // read as a path. Text that is not a path lands inside trivially.
  return argv.every((arg) => inside(arg) && (!arg.includes('=') || inside(arg.slice(arg.indexOf('=') + 1))));
}
