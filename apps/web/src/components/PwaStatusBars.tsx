import { useEffect, useState } from 'react';
import { RefreshCw, WifiOff } from 'lucide-react';
import { isOffline, listenOfflineChange, registerServiceWorker } from '../pwa.js';
import { gahApi } from '../api/client.js';

/** Issue #534: PWA plumbing UI — offline banner ("read-only, stale") and the
 * update prompt when a new deploy's service worker is waiting. Both render
 * only when relevant; neither blocks the dashboard. */
export function PwaStatusBars({ connected = false }: { connected?: boolean }) {
  const [networkOffline, setOffline] = useState(isOffline());
  const [apiReachable, setApiReachable] = useState(false);
  const offline = networkOffline && !(connected && apiReachable);
  const [updateApply, setUpdateApply] = useState<(() => void) | null>(null);

  useEffect(() => {
    setOffline(isOffline());
    const stop = listenOfflineChange(setOffline);
    registerServiceWorker(setUpdateApply);
    return stop;
  }, []);

  useEffect(() => {
    let current = true;
    setApiReachable(false);
    // An existing socket can outlive a network change. A fresh REST response
    // also lets an offline laptop use a dashboard running on loopback.
    if (networkOffline && connected) {
      void gahApi.getCoordinatorInfo().then(() => {
        if (current) setApiReachable(true);
      }).catch(() => { /* The offline banner stays visible. */ });
    }
    return () => { current = false; };
  }, [networkOffline, connected]);

  return (
    <>
      {offline && (
        <div
          role="status"
          className="flex shrink-0 items-center gap-2 border-b border-warning/30 bg-warning/15 px-4 py-2 text-sm text-warning lg:col-span-2"
        >
          <WifiOff size={14} aria-hidden="true" />
          Offline — the dashboard is read-only and the data you see may be
          stale. Queued changes are not possible; reconnect to act.
        </div>
      )}
      {updateApply && !offline && (
        <div
          role="status"
          className="flex shrink-0 items-center gap-2 border-b border-subtle bg-raised px-4 py-2 text-sm text-secondary lg:col-span-2"
        >
          Update available.
          <button
            type="button"
            onClick={() => {
              setUpdateApply(null);
              updateApply();
            }}
            className="inline-flex items-center gap-1.5 px-2 py-1 bg-accent-fill text-on-fill rounded text-xs font-medium hover:bg-accent-fill/90"
          >
            <RefreshCw size={12} aria-hidden="true" />
            Reload to update
          </button>
        </div>
      )}
    </>
  );
}
