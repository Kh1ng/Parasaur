import policy from './agyPoolIdentity.json' with { type: 'json' };

/** Shared declarative identity policy consumed by Rust routing and this observer.
 * Known provider buckets use the same pools even without a local CLI catalog;
 * new groups retain their provider identity until the policy recognizes them. */
export function agyPool(model: string, ids: string[] = []): string {
  const bucket = ids.slice().sort()[0];
  const rule = policy.pools.find(rule => bucket !== undefined
    ? rule.bucketIds.includes(bucket) : model.trim().toLowerCase().startsWith(rule.modelPrefix));
  return rule?.id ?? bucket!;
}
export const agyFallbackPools = policy.pools.map(rule => rule.id);
export const agyLabel = (value: unknown): value is string => typeof value === 'string'
  && value.length > 0 && value.length <= policy.identifierMaxLength
  && [...value].every(character => policy.identifierCharacters.includes(character));

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
