/**
 * Public deployment base path (#1593): '' when Parasaur is served at the
 * root of its host, '/parasaur' when GitLab's nginx proxies it under a path
 * prefix. The server injects `window.__GAH_BOOTSTRAP__` into every served
 * document; the values are re-validated here so a tampered, stale, or
 * missing injection falls back to the root deployment instead of building
 * asset, API, or WebSocket URLs somewhere else.
 */

export interface GahBootstrap {
  basePath: string;
  /** Origins allowed to embed and talk to the GitLab panel view. */
  gitLabOrigins: string[];
}

const BOOTSTRAP_KEY = '__GAH_BOOTSTRAP__';

/** Same strict shape the server validates before injecting (embedView.ts). */
function validBasePath(value: unknown): string {
  if (typeof value !== 'string') return '';
  const trimmed = value.trim();
  if (trimmed.length === 0 || trimmed.length > 128) return '';
  if (!/^\/[A-Za-z0-9._~-]+(?:\/[A-Za-z0-9._~-]+)*$/.test(trimmed)) return '';
  if (trimmed.split('/').some(segment => segment === '.' || segment === '..')) return '';
  return trimmed;
}

function validOriginList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const origins: string[] = [];
  for (const entry of value) {
    if (typeof entry !== 'string' || entry.includes('*') || origins.length >= 8) continue;
    try {
      const url = new URL(entry);
      // Same strict shape the server validates before injecting
      // (embedView.ts): exactly http/https, no wildcard host, no
      // credentials, no path. Anything else never authorizes an origin.
      if ((url.protocol !== 'http:' && url.protocol !== 'https:') || url.username || url.password
        || url.pathname !== '/' || url.search !== '' || url.hash !== '' || url.hostname.includes('*')) continue;
      if (!origins.includes(url.origin)) origins.push(url.origin);
    } catch {
      // An unparseable entry never authorizes an origin.
    }
  }
  return origins;
}

/** The injected deployment configuration, validated on every read. */
export function readBootstrap(): GahBootstrap {
  const raw = typeof window !== 'undefined'
    ? (window as unknown as Record<string, unknown>)[BOOTSTRAP_KEY]
    : undefined;
  const source = raw && typeof raw === 'object'
    ? raw as { basePath?: unknown; gitLabOrigins?: unknown }
    : {};
  return { basePath: validBasePath(source.basePath), gitLabOrigins: validOriginList(source.gitLabOrigins) };
}

/** The deployment's public base path ('' at the root, '/parasaur' under a prefix). */
export function basePath(): string {
  return readBootstrap().basePath;
}

/** Prefix an app-absolute path ('/api/…', '/ws') with the base path. */
export function withBasePath(path: string): string {
  const base = basePath();
  if (base === '' || !path.startsWith('/')) return path;
  return `${base}${path}`;
}
