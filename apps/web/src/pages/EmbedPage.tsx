import { useEffect, useMemo, useRef, useState, lazy, Suspense } from 'react';
import { CircleDot, ExternalLink, FileText, GitPullRequest, X } from 'lucide-react';
import { useUiStore } from '../store/uiStore.js';
import { useWebSocket } from '../ws/WebSocketContext.js';
import { gahApi, pairingApi, GahApiError } from '../api/client.js';
import { saveCoordinatorToken, requestSessionHandoff } from '../api/coordinatorToken.js';
import { ExternalAnchor } from '../components/ExternalAnchor.js';
import { LoadingState } from '../components/ui/EmptyState.js';
import { formatChatName } from '../lib/format.js';
import { useAutoRefresh } from '../hooks/useAutoRefresh.js';
import { withBasePath } from '../lib/basePath.js';
import {
  EMBED_CONTEXT_KIND_LABELS as KIND_LABELS,
  embedAllowedOrigins,
  embedContextParams,
  handleEmbedMessage,
  matchEmbedProfile,
  parseEmbedContext,
  postEmbedMessage,
  type EmbedContext,
  type EmbedContextKind
} from '../lib/embed.js';
import type { ChatSessionSummary, ProfileSummary } from '@git-agent-harness/contracts';

// The chat is the embed view's bulk; it loads lazily so the dashboard's
// entry bundle does not carry it.
const ManagerChatPage = lazy(() => import('./ManagerChatPage.js').then((module) => ({ default: module.ManagerChatPage })));

function EmbeddedChat({ context }: { context: EmbedContext | null }) {
  return (
    <Suspense fallback={<LoadingState label="Loading chat…" />}>
      <ManagerChatPage docked gitlabContext={context} />
    </Suspense>
  );
}

/**
 * The GitLab panel's embed view (#1593). `EmbedApp` is the chrome-less root
 * served at `/embed`: it tracks the page context (query string plus the
 * panel's postMessage handshake), gates on Parasaur's normal signed-in
 * session, and renders the same surface the standalone URL shows with app
 * chrome. `EmbedSurface` is that surface: context actions, the runs for
 * this issue or merge request, and the agent chat with its model and
 * skills pickers, scoped to the matching Parasaur project.
 */

const KIND_ICONS: Record<EmbedContextKind, typeof CircleDot> = {
  issue: CircleDot,
  merge_request: GitPullRequest,
  file: FileText,
  tree: FileText,
  pipeline: FileText,
  job: FileText
};

/** First turns the context actions send, mirroring the events endpoint's
 * dispatch modes (#1592): issue work, MR review, review-comment fixes. */
const CONTEXT_INSTRUCTIONS = {
  workIssue: 'Work on this issue and open a draft merge request when the change is ready.',
  reviewMr: 'Review this merge request: verify the change, flag problems, and suggest concrete improvements.',
  fixReviewComments: 'Address the unresolved review comments on this merge request.',
  explainDiff: 'Explain what this merge request changes and why.'
} as const;

/** The standalone ("open in Parasaur") URL for the same context. */
function standaloneUrl(context: EmbedContext): string {
  return `${withBasePath('/')}?${embedContextParams(context).toString()}`;
}

function contextLabel(context: EmbedContext): string {
  const parts = [context.project, context.kind ? KIND_LABELS[context.kind] : null, context.iid !== null ? `#${context.iid}` : null];
  return parts.filter(part => part !== null).join(' · ') || context.gitlab;
}

function EmbedContextHeader({ context, standalone, onDismiss }: { context: EmbedContext; standalone: boolean; onDismiss?: () => void }) {
  const KindIcon = context.kind ? KIND_ICONS[context.kind] : null;
  return (
    <header className="flex items-center gap-2 border-b border-subtle px-3 py-2">
      {KindIcon && <KindIcon size={14} className="shrink-0 text-muted" aria-hidden="true" />}
      <h1 className="min-w-0 flex-1 truncate text-sm font-semibold text-primary" title={context.url}>{contextLabel(context)}</h1>
      <ExternalAnchor href={standaloneUrl(context)} className="inline-flex min-h-9 min-w-9 items-center justify-center rounded-md text-muted hover:text-primary"
        aria-label="Open in Parasaur" title="Open the same view in Parasaur (new tab)">
        <ExternalLink size={14} aria-hidden="true" />
      </ExternalAnchor>
      {standalone ? (
        onDismiss && (
          <button type="button" onClick={onDismiss} className="inline-flex min-h-9 min-w-9 items-center justify-center rounded-md text-muted hover:text-primary"
            aria-label="Leave the GitLab context view" title="Back to the dashboard">
            <X size={15} aria-hidden="true" />
          </button>
        )
      ) : (
        <button type="button" onClick={() => postEmbedMessage({ type: 'parasaur:close' }, embedAllowedOrigins())}
          className="inline-flex min-h-9 min-w-9 items-center justify-center rounded-md text-muted hover:text-primary"
          aria-label="Close the Parasaur panel" title="Close">
          <X size={15} aria-hidden="true" />
        </button>
      )}
    </header>
  );
}

/** Runs for this context: live or settled chat sessions the profile opened
 * for this issue or merge request. Selecting one opens it in the chat
 * below, the same selection path the dashboard uses. */
function ContextRuns({ profile, context }: { profile: ProfileSummary | null; context: EmbedContext }) {
  const openChatSession = useUiStore((state) => state.openChatSession);
  const { reconnectSeq } = useWebSocket();
  const [sessions, setSessions] = useState<ChatSessionSummary[] | null>(null);
  const refresh = () => {
    if (!profile || context.iid === null || (context.kind !== 'issue' && context.kind !== 'merge_request')) return;
    gahApi.getChatSessions(profile.name)
      .then(({ sessions: fetched }) => setSessions(fetched.filter(session =>
        context.kind === 'issue' ? session.issueNumber === context.iid : session.prNumber === context.iid)))
      .catch(() => { /* The strip keeps its last list; the chat below shows errors. */ });
  };
  useEffect(refresh, [profile?.name, context.kind, context.iid, reconnectSeq]); // eslint-disable-line react-hooks/exhaustive-deps
  useAutoRefresh(refresh, 5_000);
  if (!profile || context.iid === null || (context.kind !== 'issue' && context.kind !== 'merge_request') || sessions === null) return null;
  const relevant = sessions.filter(session => session.outcome !== 'archived').slice(0, 5);
  if (relevant.length === 0) return null;
  return (
    <section aria-label="Runs for this context" className="border-b border-subtle px-3 py-2">
      <h2 className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-muted">Runs</h2>
      <ul className="space-y-1">
        {relevant.map((session) => (
          <li key={session.id}>
            <button type="button" onClick={() => openChatSession(profile.name, session.id)} className="w-full truncate rounded-md px-2 py-1 text-left text-xs text-secondary hover:bg-overlay/5"
              title={session.title ?? session.id}>
              {formatChatName(session)}
              {session.outcome === 'live' ? '' : ` · ${session.outcome}`}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** The shared surface: context header, context actions, this context's runs,
 * and the agent chat scoped to the matching Parasaur project. */
export function EmbedSurface({ context, standalone, onDismiss }: { context: EmbedContext; standalone: boolean; onDismiss?: () => void }) {
  const openChatSession = useUiStore((state) => state.openChatSession);
  const setProfileOverride = useUiStore((state) => state.setProfileOverride);
  const { reconnectSeq } = useWebSocket();
  const [profiles, setProfiles] = useState<ProfileSummary[] | null>(null);
  const [profilesError, setProfilesError] = useState<string | null>(null);
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    gahApi.getProfiles()
      .then((fetched) => { if (!cancelled) { setProfiles(fetched); setProfilesError(null); } })
      .catch((error) => { if (!cancelled) setProfilesError(error instanceof Error ? error.message : String(error)); });
    return () => { cancelled = true; };
  }, [reconnectSeq]);

  const matched = useMemo(() => profiles === null ? undefined : matchEmbedProfile(profiles, context), [profiles, context]);

  // Bind the ordinary chat to the matched project — the context actions and
  // run strip already are — including after context updates: a page change
  // that matches another project must carry the chat with it, not leave an
  // unrelated selection behind. Without a unique match the chat's own project
  // picker stays in control (the "no matching project" guidance says so).
  const matchedName = matched?.name ?? null;
  useEffect(() => {
    if (matchedName !== null) setProfileOverride(matchedName);
  }, [matchedName, setProfileOverride]);

  /** A context action starts the conversation the page context names
   * (issue chat or read-only MR chat) and sends its instruction as the
   * first turn. */
  const runContextAction = async (action: keyof typeof CONTEXT_INSTRUCTIONS) => {
    if (!matched || actionBusy || context.iid === null) return;
    setActionBusy(true);
    setActionError(null);
    try {
      const settings = await gahApi.getManagerChatSettings();
      const backend = settings.profileOverrides[matched.name] ?? settings.defaultBackend;
      const { session } = action === 'workIssue'
        ? await gahApi.startChatFromIssue(matched.name, context.iid, backend, null)
        : await gahApi.startChatFromPr(matched.name, context.iid, backend, null);
      openChatSession(matched.name, session.id, CONTEXT_INSTRUCTIONS[action]);
    } catch (error) {
      setActionError(error instanceof Error ? error.message : String(error));
    } finally {
      setActionBusy(false);
    }
  };

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
      <EmbedContextHeader context={context} standalone={standalone} onDismiss={onDismiss} />
      {matched === undefined && !profilesError && <LoadingState label="Matching project…" />}
      {profilesError && <p role="alert" className="px-3 py-2 text-xs text-critical">Could not load projects: {profilesError}</p>}
      {matched === null && (
        <p className="px-3 py-2 text-xs text-secondary">
          No Parasaur project matches {context.project ?? 'this GitLab page'}. Import it from the dashboard, or pick a project in the chat below.
        </p>
      )}
      {matched && (
        <>
          <div className="flex flex-wrap items-center gap-1.5 border-b border-subtle px-3 py-2">
            {context.kind === 'issue' && context.iid !== null && (
              <button type="button" onClick={() => void runContextAction('workIssue')} disabled={actionBusy}
                className="btn-primary !min-h-9 text-xs">Work on this issue</button>
            )}
            {context.kind === 'merge_request' && context.iid !== null && (
              <>
                <button type="button" onClick={() => void runContextAction('reviewMr')} disabled={actionBusy}
                  className="btn-primary !min-h-9 text-xs">Review this MR</button>
                <button type="button" onClick={() => void runContextAction('fixReviewComments')} disabled={actionBusy}
                  className="btn-secondary !min-h-9 text-xs">Fix review comments</button>
                <button type="button" onClick={() => void runContextAction('explainDiff')} disabled={actionBusy}
                  className="btn-secondary !min-h-9 text-xs">Explain diff</button>
              </>
            )}
          </div>
          <ContextRuns profile={matched} context={context} />
        </>
      )}
      {actionError && <p role="alert" className="px-3 py-2 text-xs text-critical">{actionError}</p>}
      <main className="flex min-h-0 flex-1 flex-col overflow-hidden px-1 pb-1">
        {/* The ordinary chat carries the resolved page context with its
            outgoing turns, so the conversation knows which GitLab page it
            serves even without a context action. */}
        <EmbeddedChat context={context} />
      </main>
    </div>
  );
}

/** The `/embed` route's root: no app shell, the panel postMessage
 * handshake, and Parasaur's normal sign-in gate. */
export function EmbedApp() {
  const [context, setContext] = useState<EmbedContext | null>(() => parseEmbedContext(window.location.search));
  const [signedOut, setSignedOut] = useState(false);
  const { isConnected } = useWebSocket();
  const checkCounter = useRef(0);

  const checkSession = () => {
    const check = ++checkCounter.current;
    pairingApi.session()
      .then(() => { if (check === checkCounter.current) setSignedOut(false); })
      .catch((error) => { if (check === checkCounter.current && error instanceof GahApiError && error.status === 401) setSignedOut(true); });
  };
  useEffect(checkSession, [isConnected]); // eslint-disable-line react-hooks/exhaustive-deps

  // #1593 round 2: a sign-in in the new tab the prompt opens must reach this
  // iframe, which lives in its own browsing context and so cannot see that
  // tab's sessionStorage. While the prompt shows, the iframe re-checks the
  // auth endpoint (device pairing lands as a same-origin cookie, which this
  // context already carries) and listens for the same-origin session handoff
  // an owner-token sign-in publishes. The credential never passes through
  // the embedding page; after pickup the view re-renders signed in.
  useEffect(() => {
    if (!signedOut) return;
    const stop = requestSessionHandoff((token) => {
      if (token) saveCoordinatorToken(token);
      checkSession();
    });
    const timer = window.setInterval(checkSession, 3_000);
    return () => {
      stop();
      window.clearInterval(timer);
    };
  }, [signedOut]); // eslint-disable-line react-hooks/exhaustive-deps

  // The panel handshake: announce readiness (the panel replies with the
  // context), and take context updates as the GitLab page changes.
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      const message = handleEmbedMessage(event, embedAllowedOrigins());
      if (message !== null) setContext(message.context);
    };
    window.addEventListener('message', onMessage);
    postEmbedMessage({ type: 'parasaur:ready' }, embedAllowedOrigins());
    return () => window.removeEventListener('message', onMessage);
  }, []);

  return (
    <div className="app-shell flex h-dvh flex-col overflow-hidden bg-page">
      {context ? (
        <EmbedSurface context={context} standalone={false} />
      ) : (
        <>
          <header className="flex items-center gap-2 border-b border-subtle px-3 py-2">
            <h1 className="min-w-0 flex-1 truncate text-sm font-semibold text-primary">Parasaur</h1>
          </header>
          <p className="px-3 py-2 text-xs text-secondary">
            Waiting for the GitLab page context — open this panel from a GitLab page.
          </p>
          <main className="flex min-h-0 flex-1 flex-col overflow-hidden px-1 pb-1">
            <EmbeddedChat context={context} />
          </main>
        </>
      )}
      {signedOut && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-page/95 p-6" role="dialog" aria-label="Sign in to Parasaur">
          <div className="max-w-sm space-y-3 text-center">
            <h2 className="text-base font-semibold text-primary">Sign in to Parasaur</h2>
            <p className="text-sm text-secondary">
              The GitLab panel uses Parasaur's normal dashboard session. Open Parasaur, sign in with your owner token or pairing link, then come back — this panel picks the session up by itself.
            </p>
            <div className="flex justify-center gap-2">
              <ExternalAnchor href={context ? standaloneUrl(context) : withBasePath('/')} className="btn-primary !min-h-11 text-sm">
                Open Parasaur to sign in
              </ExternalAnchor>
              <button type="button" className="btn-secondary !min-h-11 text-sm" onClick={checkSession}>Retry</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
