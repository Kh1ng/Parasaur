/** Provider bucket IDs identify a shared group, independent of model names.
 * Keep this ordering identical to src/availability/agy.rs. */
let warned = false;
export function agyGroupPool(ids: unknown[]): string | undefined {
  if (!ids.length || new Set(ids).size !== ids.length || ids.some(id => typeof id !== 'string' || !/^[a-zA-Z0-9_.-]{1,100}$/.test(id))) {
    if (!warned) {
      warned = true;
      console.warn('[cliRouter] Unrecognized AGY quota metadata; using an account-scoped default pool.');
    }
    return undefined;
  }
  return (ids as string[]).slice().sort()[0];
}
