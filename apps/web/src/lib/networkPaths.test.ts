import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';

const sourceRoot = join(__dirname, '..');
function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? sourceFiles(path) : /\.[cm]?[jt]sx?$/.test(path) && !path.endsWith('.test.ts') ? [path] : [];
  });
}

// Grep-style guard: app-absolute literals must pass through the URL helpers.
const bypass = /\bfetch\s*\(\s*['"`]\/api|\bnew\s+(?:WebSocket|EventSource)\s*\(\s*['"`]\//;
test('network calls never bypass the deployment prefix with a root literal', () => {
  expect(sourceFiles(sourceRoot).filter(path => bypass.test(readFileSync(path, 'utf8')))
    .map(path => relative(sourceRoot, path))).toEqual([]);
});

test('the guard recognizes single, double and template quoted bypasses', () => {
  for (const quote of ["'", '"', '`']) {
    expect(bypass.test(`fetch(${quote}/api/settings${quote})`)).toBe(true);
    expect(bypass.test(`new WebSocket(${quote}/ws${quote})`)).toBe(true);
    expect(bypass.test(`new EventSource(${quote}/api/activity${quote})`)).toBe(true);
  }
});
