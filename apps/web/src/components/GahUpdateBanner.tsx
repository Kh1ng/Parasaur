import { useEffect, useRef, useState } from 'react';
import { RefreshCw, X } from 'lucide-react';
import { gahApi, GahApiError } from '../api/client.js';
import type { AdminUpdateState, ReleaseChannelStatus } from '@git-agent-harness/contracts';

const RELEASE_POLL_MS = 15 * 60_000;
const UPDATE_STATUS_POLL_MS = 2_000;

/**
 * Non-blocking central-server release notice. Persisted running updates resume
 * observation after reconnect; polling belongs to this component's lifetime.
 * Checks the release channel periodically and starts artifact installation
 * (`--from-release`, no rebuild). Completion or an inferred restart reloads
 * the dashboard. Persisted failures remain visible even without a newer feed
 * entry; disabled admin updates render nothing.
 */
export function GahUpdateBanner() {
  const [enabled, setEnabled] = useState(true);
  const [release, setRelease] = useState<ReleaseChannelStatus | null>(null);
  const [status, setStatus] = useState<AdminUpdateState | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const startingRequest = useRef(false);
  const mounted = useRef(true);
  const updateRequested = useRef(false);
  const [statusUnavailable, setStatusUnavailable] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const load = () => {
      gahApi
        .getReleaseStatus()
        .then((data) => { if (!cancelled) { setRelease(data); setError(null); setEnabled(true); } })
        .catch((err) => {
          if (cancelled) return;
          if (err instanceof GahApiError && err.status === 404) {
            setEnabled(false);
            return;
          }
          // Channel unreachable (offline, GitHub hiccup): stay quiet rather
          // than nagging about a feed that cannot be read.
          setError(err instanceof Error ? err.message : String(err));
        });
    };
    load();
    const timer = setInterval(load, RELEASE_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    gahApi.getAdminUpdateStatus().then((state) => {
      // A slow initial read must not overwrite an update started meanwhile.
      if (!cancelled && !updateRequested.current) setStatus(state);
    }).catch(() => undefined);
    return () => { cancelled = true; mounted.current = false; };
  }, []);

  const running = status?.status === 'running';
  useEffect(() => {
    if (!running) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => {
      try {
        const next = await gahApi.getAdminUpdateStatus();
        if (cancelled) return;
        setStatus(next);
        setStatusUnavailable(false);
        if (next.status !== 'running') {
          if (next.status === 'success' || next.status === 'inferred_restart') window.location.reload();
          return;
        }
      } catch {
        if (cancelled) return;
        // A restart can interrupt reads. Retain the last state as unconfirmed.
        setStatusUnavailable(true);
      }
      timer = setTimeout(tick, UPDATE_STATUS_POLL_MS);
    };
    timer = setTimeout(tick, UPDATE_STATUS_POLL_MS);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [running]);

  const restartToUpdate = async () => {
    if (startingRequest.current || running) return;
    startingRequest.current = true;
    updateRequested.current = true;
    setStarting(true);
    setError(null);
    try {
      const state = await gahApi.startAdminUpdate('release');
      if (mounted.current) { setStatus(state); setStatusUnavailable(false); }
    } catch (err) {
      if (mounted.current) setError(err instanceof Error ? err.message : String(err));
    } finally {
      startingRequest.current = false;
      if (mounted.current) setStarting(false);
    }
  };

  const failed = status?.status === 'failed';
  if (!enabled || dismissed || (!release?.update_available && !running && !failed)) return null;

  const current = release?.current_version;
  const latest = release?.latest_version ?? 'newer';

  return (
    <div
      role="status"
      className="flex shrink-0 flex-wrap items-center gap-2 border-b border-accent/30 bg-accent/10 px-4 py-2 text-sm text-primary lg:col-span-2"
    >
      <RefreshCw size={14} aria-hidden="true" className="text-accent" />
      <span className="font-medium break-words">{failed ? 'Server update failed' : release?.update_available ? `Server update available · v${current} → v${latest}` : 'Server update in progress'}</span>
      {running && <span className="text-secondary">{statusUnavailable ? 'Server update status unavailable. Reconnecting…' : 'Installing release and restarting server…'}</span>}
      {failed && <span className="text-secondary">Review the update log in Settings before retrying.</span>}
      {!running && (
        <button
          type="button"
          onClick={() => void restartToUpdate()}
          disabled={starting}
          className="inline-flex items-center gap-1.5 rounded bg-accent-fill px-2 py-1 text-xs font-medium text-on-fill hover:bg-accent-fill/90 disabled:opacity-50"
        >
          {starting ? 'Starting…' : failed ? 'Retry server update' : 'Update server'}
        </button>
      )}
      <a className="text-xs text-accent underline underline-offset-2" href="#page=settings" onClick={() => setDismissed(true)}>
        Details in Settings
      </a>
      <button
        type="button"
        aria-label="Dismiss update banner"
        className="ml-auto text-secondary hover:text-primary"
        onClick={() => setDismissed(true)}
      >
        <X size={14} aria-hidden="true" />
      </button>
      {error && <span className="text-xs text-critical">{error}</span>}
    </div>
  );
}
