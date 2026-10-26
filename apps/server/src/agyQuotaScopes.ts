import { mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { createHash, randomUUID } from 'node:crypto';
import { agyPool, agyLabel } from './agyQuota.js';
import { dirname } from 'node:path';

type Scope = { quotaPool: string; window?: string };
const digest = (key: string) => createHash('sha256').update(key).digest('hex');

/** Retain only provider scope identities across restarts, never balances or credentials. */
export class AgyQuotaScopes {
  private readonly scopes = new Map<string, Scope[]>();
  constructor(private readonly path: string) {
    try {
      const raw = readFileSync(path, 'utf8');
      if (raw.length > 4 * 1024 * 1024) return;
      const entries: unknown = JSON.parse(raw);
      if (!Array.isArray(entries)) return;
      for (const entry of entries.slice(-64)) {
        if (!Array.isArray(entry) || entry.length !== 2) continue;
        const [key, scopes] = entry;
        if (typeof key !== 'string' || !/^[a-f0-9]{64}$/.test(key) || !Array.isArray(scopes) || scopes.length > 128) continue;
        if (scopes.every(scope => scope && agyLabel(scope.quotaPool) && (scope.window === undefined || agyLabel(scope.window)))) {
          this.scopes.set(key, scopes.map(scope => ({ quotaPool: agyPool('', [scope.quotaPool]), ...(scope.window ? { window: scope.window } : {}) })));
        }
      }
    } catch { /* Missing or malformed state is rebuilt by successful refreshes. */ }
  }
  get(key: string): Scope[] | undefined { return this.scopes.get(digest(key)); }
  set(key: string, scopes: { quotaPool?: string; window?: string }[]): void {
    const id = digest(key);
    this.scopes.delete(id);
    this.scopes.set(id, scopes.filter((scope): scope is Scope => agyLabel(scope.quotaPool)).slice(0, 128));
    while (this.scopes.size > 64) this.scopes.delete(this.scopes.keys().next().value!);
    const tmp = `${this.path}.${randomUUID()}.tmp`;
    try {
      mkdirSync(dirname(this.path), { recursive: true, mode: 0o700 });
      writeFileSync(tmp, JSON.stringify([...this.scopes]), { mode: 0o600 });
      renameSync(tmp, this.path);
    } catch {
      console.warn('[cliRouter] AGY quota scopes could not be saved for restart recovery.');
    } finally { rmSync(tmp, { force: true }); }
  }
}
