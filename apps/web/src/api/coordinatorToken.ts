const TOKEN_KEY = 'gah.coordinatorToken';
export const TOKEN_CHANGED_EVENT = 'gah.coordinatorTokenChanged';
// #1593 round 2: the embed iframe lives in its own browsing context, so it
// cannot read the sign-in tab's sessionStorage. While it waits for a session
// it marks that here; any later saveCoordinatorToken in another same-origin
// Parasaur tab then mirrors the credential through same-origin localStorage
// (never through the embedding page), and the waiting view adopts it into
// its own sessionStorage and clears the handoff.
const HANDOFF_KEY = 'gah.coordinatorTokenHandoff';
const WANTED_KEY = 'gah.coordinatorTokenWanted';

export function coordinatorToken(): string {
  try { return typeof window === 'undefined' ? '' : window.sessionStorage.getItem(TOKEN_KEY) ?? ''; }
  catch { return ''; }
}

/** Keep REST and live connections on the same tab-scoped credential. */
export function saveCoordinatorToken(token: string): void {
  if (token) window.sessionStorage.setItem(TOKEN_KEY, token);
  else window.sessionStorage.removeItem(TOKEN_KEY);
  window.dispatchEvent(new Event(TOKEN_CHANGED_EVENT));
  publishSessionHandoff(token);
}

/** Mirror the credential to a waiting embed view in another browsing
 * context. No-op when none is waiting: a normal dashboard sign-in never
 * writes anything to localStorage. */
function publishSessionHandoff(token: string): void {
  try {
    if (window.localStorage.getItem(WANTED_KEY) !== '1') return;
    if (token) window.localStorage.setItem(HANDOFF_KEY, token);
    else window.localStorage.removeItem(HANDOFF_KEY);
  } catch {
    // Storage unavailable (disabled, quota): the waiting view keeps
    // polling the auth endpoint; cookie-based sessions still reach it.
  }
}

/** An embed view showing its sign-in prompt calls this while it waits for a
 * session established elsewhere (the new tab the prompt opens). The
 * credential never passes through the embedding page: the handoff lives in
 * same-origin storage, which a cross-origin embedder cannot read, and a
 * same-origin one can already read the iframe's sessionStorage directly.
 * Returns a cleanup function that stops waiting. */
export function requestSessionHandoff(onHandoff: (token: string) => void): () => void {
  try {
    window.localStorage.setItem(WANTED_KEY, '1');
  } catch {
    // Storage unavailable: polling the auth endpoint still covers
    // cookie-based sign-ins.
  }
  const adopt = () => {
    try {
      const token = window.localStorage.getItem(HANDOFF_KEY);
      if (token === null) return;
      window.localStorage.removeItem(HANDOFF_KEY);
      window.localStorage.removeItem(WANTED_KEY);
      onHandoff(token);
    } catch {
      // Nothing to adopt from unusable storage.
    }
  };
  const onStorage = (event: StorageEvent) => {
    if (event.key === HANDOFF_KEY || event.key === null) adopt();
  };
  window.addEventListener('storage', onStorage);
  // A handoff published between this view mounting and the listener
  // registering never fired an event here: adopt whatever already waits.
  adopt();
  return () => {
    window.removeEventListener('storage', onStorage);
    try {
      window.localStorage.removeItem(WANTED_KEY);
      window.localStorage.removeItem(HANDOFF_KEY);
    } catch { /* Nothing waits for a handoff anymore. */ }
  };
}

/** Keep credentials out of request URLs; the server echoes only gah.v1. */
export function coordinatorWebSocketProtocols(): string[] {
  const token = coordinatorToken();
  if (!token) return ['gah.v1'];
  const encoded = btoa(Array.from(new TextEncoder().encode(token), byte => String.fromCharCode(byte)).join(''))
    .replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  return ['gah.v1', `gah-auth.${encoded}`];
}
