import { useEffect, useRef, useState } from 'react';
import type { ProfileSummary } from '@git-agent-harness/contracts';
import { gahApi } from '../api/client.js';
import { appUrl, deployment } from '../lib/deployment.js';
import { contextText, matchesContextRun, navigationPath, parseGitlabContext } from '../lib/gitlabContext.js';
import { useUiStore } from '../store/uiStore.js';
import { useWebSocket } from '../ws/WebSocketContext.js';
import { ManagerChatPage } from './ManagerChatPage.js';

export function EmbedPage({ standalone = false }: { standalone?: boolean }) {
  const [origins] = useState(() => deployment().gitlabOrigins);
  const [context, setContext] = useState(() => parseGitlabContext(Object.fromEntries(new URLSearchParams(location.search)), origins));
  const [project, setProject] = useState<ProfileSummary | null>(null);
  const [error, setError] = useState('');
  const [chatId, setChatId] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const contextRef = useRef(context);
  contextRef.current = context;
  const [action, setAction] = useState({ text: '', revision: 0 });
  const { sessions, isConnected } = useWebSocket();
  const setProfileOverride = useUiStore(state => state.setProfileOverride);
  useEffect(() => { if (project) setProfileOverride(project.name); }, [project, setProfileOverride]);
  const send = (type: string, extra = {}) => {
    if (context && !standalone) window.parent.postMessage({ type, ...extra }, context.gitlab);
  };
  useEffect(() => {
    const receive = (event: MessageEvent) => {
      if (event.source !== window.parent || !origins.includes(event.origin) || event.data?.type !== 'parasaur:context') return;
      const next = parseGitlabContext(event.data.context, origins);
      if (next?.gitlab === event.origin) setContext(previous => JSON.stringify(previous) === JSON.stringify(next) ? previous : next);
    };
    window.addEventListener('message', receive);
    // An allowlisted parent can supply context after the initial ready handshake.
    if (!standalone) for (const origin of origins) window.parent.postMessage({ type: 'parasaur:ready' }, origin);
    return () => window.removeEventListener('message', receive);
  }, [origins, standalone]);
  useEffect(() => {
    let active = true;
    setProject(null);
    setChatId(null);
    setStarting(false);
    setAction({ text: '', revision: 0 });
    setError('');
    if (!context) return;
    Promise.all([gahApi.getProfiles(), gahApi.getProjects()]).then(([profiles, projects]) => {
      if (!active) return;
      const match = [...projects, ...profiles].find(p => p.provider === 'gitlab' && p.repo === context.project && p.web_url?.replace(/\/$/, '') === `${context.gitlab}/${context.project}`);
      if (match) setProject({ ...match, name: 'chat_profile' in match ? String(match.chat_profile) : match.name });
      else setError('Import this GitLab project into Parasaur to start chatting.');
    }).catch(() => { if (active) setError('Could not load projects. Check your connection and authentication.'); });
    return () => { active = false; };
  }, [context]);
  const startChat = async (text = '') => {
    if (!context || !project || starting) return;
    setAction(previous => ({ text, revision: previous.revision + 1 }));
    if (chatId) return;
    const requestedContext = context;
    setStarting(true);
    try {
      // Starting a worktree is an explicit user action, never an iframe load effect.
      const session = await gahApi.createChatSession(project.name, undefined, undefined, `${context.project} ${context.kind} ${context.iid || context.ref || context.path}`);
      if (contextRef.current === requestedContext) setChatId(session.id);
    } catch {
      if (contextRef.current === requestedContext) setError('Could not start the context chat. Try again.');
    } finally {
      if (contextRef.current === requestedContext) setStarting(false);
    }
  };
  const navigate = (path: string) => {
    if (!context) return;
    const safe = navigationPath(path, context.gitlab);
    if (safe) send('parasaur:navigate', { path: safe });
  };
  const standaloneUrl = `${appUrl('/embed')}?${new URLSearchParams({ ...context, standalone: '1' })}`;
  const runs = sessions.filter(run => project && context && matchesContextRun(run, context));
  return <div className={`flex flex-col bg-page text-primary ${standalone ? 'min-h-0 flex-1' : 'h-dvh'}`}>
    <main className="flex min-h-0 flex-1 flex-col gap-3 p-3">
      <div className="flex flex-wrap gap-3">
        <span>{context ? `${context.project} · ${context.kind} ${context.iid}` : 'Waiting for trusted GitLab page context'}</span>
        {!standalone && <><a href={standaloneUrl} target="_blank" rel="noopener noreferrer">Open in Parasaur</a><button onClick={() => send('parasaur:close')}>Close</button></>}
        {context && <button onClick={() => navigate(new URL(context.url).pathname)}>Open GitLab page</button>}
      </div>
      {error && <p role="alert">{error}</p>}
      {context && project && <>
        <div className="flex flex-wrap gap-2" aria-label="Context actions">
          {(context.kind === 'mr' || context.kind === 'merge_request' ? ['Review this MR', 'Fix review comments', 'Explain diff'] : context.kind === 'issue' ? ['Work on this issue'] : ['Explain diff']).map(text => <button key={text} className="rounded border border-subtle p-2" disabled={starting} onClick={() => void startChat(text)}>{text}</button>)}
        </div>
        <details><summary>Runs for this context ({isConnected ? runs.length : 'unknown'})</summary>{runs.map(run => <p key={run.id}>{run.id} · {run.status}</p>)}</details>
        {!chatId ? <button disabled={starting} onClick={() => void startChat()}>{starting ? 'Starting chat…' : 'Start context chat'}</button> :
          <ManagerChatPage key={JSON.stringify(context)} docked scopedProfile={project.name} scopedSessionId={chatId} pageContext={contextText(context)} contextAction={action} />}
      </>}
    </main>
  </div>;
}
