import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';

/**
 * Under a deployment prefix such as /parasaur (#1593), a root-absolute
 * request or navigation reaches the origin root instead of Parasaur. Every
 * one goes through `apiUrl` or `withBasePath` (#1654); this fails when a new
 * call site hands a root-absolute literal straight to the network or the
 * location.
 */
const ROOT_ABSOLUTE_CALLS = [
  /\bfetch\(\s*['"`]\//,
  /\bnew\s+WebSocket\(\s*['"`]\//,
  /\bnew\s+EventSource\(\s*['"`]\//,
  /\blocation\.(?:assign|replace)\(\s*['"`]\//,
  /\blocation\.href\s*=\s*['"`]\//,
];

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

test('no source file sends a root-absolute URL past the base-path helpers', () => {
  const root = join(__dirname, '..');
  const offenders = sourceFiles(root).flatMap((file) =>
    readFileSync(file, 'utf8')
      .split('\n')
      .flatMap((line, index) =>
        ROOT_ABSOLUTE_CALLS.some((pattern) => pattern.test(line))
          ? [`${relative(root, file)}:${index + 1}: ${line.trim()}`]
          : [],
      ),
  );
  expect(offenders).toEqual([]);
});

test('the guard recognizes each root-absolute call shape', () => {
  for (const line of [
    "fetch('/api/status')",
    'new WebSocket(`/ws`)',
    'new EventSource("/api/events")',
    "window.location.assign('/?page=events')",
    "location.href = '/'",
  ]) {
    expect(ROOT_ABSOLUTE_CALLS.some((pattern) => pattern.test(line))).toBe(true);
  }
  expect(ROOT_ABSOLUTE_CALLS.some((pattern) => pattern.test('fetch(apiUrl(path).toString())'))).toBe(false);
});
