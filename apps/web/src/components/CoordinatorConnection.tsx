import { useEffect, useRef, useState } from 'react';
import { coordinatorToken, saveCoordinatorToken, TOKEN_CHANGED_EVENT } from '../api/coordinatorToken.js';
import { useWebSocket } from '../ws/WebSocketContext.js';
import { pairingApi } from '../api/client.js';
import { DevicePairing } from './DevicePairing.js';

/** Available before authenticated dashboard data loads, including in the native webview. */
export function CoordinatorConnection() {
  const { trustedLanMode, isConnected } = useWebSocket();
  const [token, setToken] = useState(coordinatorToken);
  const [error, setError] = useState('');
  const [handoffCode] = useState(() => {
    const code = new URLSearchParams(window.location.hash.slice(1)).get('gah_signin');
    return code && /^[A-Za-z0-9_-]{43}$/.test(code) ? code : null;
  });
  const [handoffApproved, setHandoffApproved] = useState(false);
  const [hasSavedToken, setHasSavedToken] = useState(() => Boolean(coordinatorToken()));
  const [handoffBusy, setHandoffBusy] = useState(false);
  useEffect(() => {
    if (handoffCode) window.history.replaceState(window.history.state, '', window.location.pathname + window.location.search);
  }, [handoffCode]);
  const approveHandoff = async () => {
    setHandoffBusy(true);
    try { await pairingApi.approveHandoff(handoffCode!); setHandoffApproved(true); setError(''); }
    catch (cause) { setError(cause instanceof Error ? cause.message : 'Cannot connect the GitLab panel.'); }
    finally { setHandoffBusy(false); }
  };
  const ownerForm = useRef<HTMLDetailsElement>(null);
  const tokenInput = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const changed = () => { setToken(coordinatorToken()); setHasSavedToken(Boolean(coordinatorToken())); };
    window.addEventListener(TOKEN_CHANGED_EVENT, changed);
    return () => window.removeEventListener(TOKEN_CHANGED_EVENT, changed);
  }, []);
  return <div className="mt-3">
    {handoffCode && <div className="mb-3 space-y-2 rounded-md border border-subtle p-3 text-sm">
      <p>{handoffApproved ? 'Sign-in approved. Return to the GitLab panel.' : 'Sign in with your owner token, then connect the GitLab panel within 60 seconds.'}</p>
      {!handoffApproved && <button type="button" className="btn-primary" disabled={!hasSavedToken || handoffBusy} onClick={() => void approveHandoff()}>Connect GitLab panel</button>}
    </div>}
    {trustedLanMode && <p role="status" className="mb-3 rounded-md border border-warning p-3 text-sm text-warning">Trusted-LAN mode is enabled. Live connections may work without a token. Remote dashboard data and session operations require pairing or an access token. Node setup requires owner access.</p>}
    <DevicePairing requestOwnerAccess={() => {
      if (ownerForm.current) ownerForm.current.open = true;
      tokenInput.current?.focus();
    }} />
    <details ref={ownerForm} className="mt-3 text-sm" open={!isConnected || undefined}>
      <summary className="cursor-pointer text-secondary"><span>Central access token</span> <span className="text-muted">(optional owner access)</span></summary>
      <form className="mt-2 flex max-w-xl flex-wrap items-end gap-2" onSubmit={event => {
        event.preventDefault();
        try { saveCoordinatorToken(token); setError(''); }
        catch { setError('Cannot save the token in this tab. Check your browser storage settings.'); }
      }}>
        <label className="min-w-0 flex-1 text-xs text-secondary">Access token
          <input ref={tokenInput} type="password" autoComplete="off" className="input mt-1 w-full" value={token} onChange={event => setToken(event.target.value)} />
        </label>
        <button className="btn-secondary" type="submit">Save and reconnect</button>
        <p className="w-full text-xs text-muted">Owner access is required to generate pairing QR codes and administer central. Controller devices can use the dashboard without this token. Tokens entered here last only for this tab. The desktop app can retain one from This computer.</p>
      </form>
    </details>
    {error && <p role="alert" className="mt-2 text-xs text-critical">{error}</p>}
  </div>;
}
