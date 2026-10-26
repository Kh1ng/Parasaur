import { createHash, timingSafeEqual } from 'node:crypto';

/** An unset secret fails closed. Fixed-size hashes hide secret length during comparison. */
export function verifyHookSecret(supplied: string | undefined, expected = process.env.GAH_GITLAB_HOOK_SECRET): boolean {
  if (!expected || supplied === undefined) return false;
  const hash = (value: string) => createHash('sha256').update(value).digest();
  return timingSafeEqual(hash(supplied), hash(expected));
}
