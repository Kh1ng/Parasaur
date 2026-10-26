import policy from './agyPoolIdentity.json' with { type: 'json' };

/** Shared declarative identity policy consumed by Rust routing and this observer.
 * Known provider buckets use the same pools even without a local CLI catalog;
 * new groups retain their provider identity until the policy recognizes them. */
export function agyPool(model: string, ids: string[] = []): string {
  const bucket = ids.slice().sort()[0];
  const rule = policy.pools.find(rule => bucket !== undefined
    ? bucket.startsWith(rule.bucketPrefix) : model.trim().toLowerCase().startsWith(rule.modelPrefix));
  return rule?.id ?? bucket!;
}
export const agyFallbackPools = policy.pools.map(rule => rule.id);
export const agyLabel = (value: unknown): value is string => typeof value === 'string' && /^[a-zA-Z0-9_.-]{1,100}$/.test(value);

let warned = false;
export function agyGroupPool(ids: unknown[]): string | undefined {
  if (!ids.length || new Set(ids).size !== ids.length || ids.some(id => !agyLabel(id))) {
    if (!warned) {
      warned = true;
      console.warn('[cliRouter] Unrecognized AGY quota metadata; retaining last-good account quota scopes.');
    }
    return undefined;
  }
  warned = false;
  return agyPool('', ids as string[]);
}
