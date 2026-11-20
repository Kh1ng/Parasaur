import { readFileSync } from 'node:fs';

export function buildCommit(value: unknown): string | null {
  return typeof value === 'string' && /^[0-9a-f]{40}$/i.test(value) ? value.toLowerCase() : null;
}

// Capture the running build once. An update can replace the files on disk
// before the service manager has restarted the old process.
export const runningBuild = (() => {
  let revision: string | null = null;
  const ancestors = new Set<string>();
  try {
    const stamp = JSON.parse(readFileSync(new URL('./build-revision.json', import.meta.url), 'utf8'));
    revision = buildCommit(stamp.revision);
    if (revision && Array.isArray(stamp.ancestors)) {
      for (const value of stamp.ancestors) {
        const sha = buildCommit(value);
        if (sha) ancestors.add(sha);
      }
    }
  } catch { /* Source installations may have no release stamp. */ }
  return { revision, ancestors };
})();
