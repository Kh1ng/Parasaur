import { useEffect, useState } from 'react';
import { FolderGit2, Cpu, X, CircleDot, GitPullRequest, SlidersHorizontal } from 'lucide-react';
import type { ChatAccountInfo, ChatIssueSummary, ChatPrSummary, ManagerModelInfo, ProfileSummary, ProjectSummary } from '@git-agent-harness/contracts';
import { ChatNodePicker } from './ChatNodePicker.js';
import { useChatNodes } from '../hooks/useChatNodes.js';
import { BoundedCollection } from './BoundedCollection.js';
import { gahApi } from '../api/client.js';
import type { ManagerBackendInfo } from '@git-agent-harness/contracts';

export type ChatProfile = ProfileSummary & Partial<Pick<ProjectSummary, 'node_id' | 'chat_profile'>> & { remote?: boolean; catalogName?: string };
export type ChatSource = 'blank' | 'issue' | 'pr';

function creationError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  if (/permission|forbidden|not authorized/i.test(message)) return `Permission denied. ${message}`;
  if (/branch.*(?:conflict|exists)|(?:conflict|exists).*branch/i.test(message)) return `Branch conflict. ${message}`;
  return message;
}

interface NewChatPanelProps {
  currentProfile: string;
  profiles: ChatProfile[];
  nodesRefreshKey?: string;
  backends: ManagerBackendInfo[];
  onClose: () => void;
  /** Blank chat: (profile, sessionId). Issue/PR chat: same shape — the
   * session is opened the same way either way. */
  onCreated: (profile: string, sessionId: string) => void;
}

/**
 * The new-chat form, shown inline in the chat panel instead of a dialog.
 * Name the chat and start, or start from an issue or pull request; the
 * project, node, provider/model and account live under "Extra settings"
 * with the current choice summarised on the disclosure. The created
 * conversation is bound to a fresh worktree — each node retains its own
 * checkout when the conversation moves. The branch survives archive/reclaim.
 *
 * "From issue" grabs a provider issue instead: the session branches for the
 * issue (`gah/issue/<repo>-<n>`), the issue is marked in progress, and the
 * conversation opens seeded with the issue body.
 *
 * "From PR" opens a read-only chat seeded with a pull request — no branch,
 * no worktree, nothing at the provider is touched.
 */
export function NewChatPanel({ currentProfile, profiles, backends, onClose, onCreated, nodesRefreshKey = '' }: NewChatPanelProps) {
  const [project, setProject] = useState(currentProfile);
  const [nodeChoice, setNodeChoice] = useState<{ project: string; nodeId: string } | null>(null);
  const [backend, setBackend] = useState<string>('');
  const [models, setModels] = useState<ManagerModelInfo[]>([]);
  const [model, setModel] = useState<string | null>(null);
  const [accountCatalog, setAccountCatalog] = useState<{ scope: string; accounts: ChatAccountInfo[] } | null>(null);
  const [accountChoice, setAccountChoice] = useState<{ scope: string; instance: string | null } | null>(null);
  const [accountFailure, setAccountFailure] = useState<{ scope: string; message: string } | null>(null);
  const [accountRetry, setAccountRetry] = useState(0);
  const [title, setTitle] = useState('');
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [query, setQuery] = useState('');
  const [mode, setMode] = useState<ChatSource>('blank');
  const [extraOpen, setExtraOpen] = useState(false);
  const [issues, setIssues] = useState<ChatIssueSummary[]>([]);
  const [issuesLoading, setIssuesLoading] = useState(false);
  const [issuesError, setIssuesError] = useState<string | null>(null);
  const [issue, setIssue] = useState<ChatIssueSummary | null>(null);
  const [prs, setPrs] = useState<ChatPrSummary[]>([]);
  const [prsLoading, setPrsLoading] = useState(false);
  const [prsError, setPrsError] = useState<string | null>(null);
  const [sourceRetry, setSourceRetry] = useState(0);
  const [pr, setPr] = useState<ChatPrSummary | null>(null);

  const nodeSnapshot = useChatNodes(project, backend || null, true, nodesRefreshKey);
  const projectInfo = profiles.find(candidate => candidate.name === project);
  const centralId = nodeSnapshot.nodes.find(node => node.role === 'central')?.nodeId ?? '';
  // Prefer the owning node, but never default to one that can't take the chat (#1275).
  const ownerId = projectInfo?.node_id;
  const ownerUsable = !!ownerId && nodeSnapshot.nodes.some(node => node.nodeId === ownerId && node.eligible);
  const nodeId = nodeChoice?.project === project ? nodeChoice.nodeId
    : ownerUsable ? ownerId : nodeSnapshot.nodes.find(node => node.eligible)?.nodeId ?? centralId;
  const selectedNode = nodeSnapshot.nodes.find(node => node.nodeId === nodeId);
  const nodeReady = !nodeSnapshot.loading && !nodeSnapshot.error && !!(selectedNode?.eligible ?? selectedNode?.chatCapable);
  const accountScope = JSON.stringify([project, backend, nodeId]);
  const backendInstances = accountCatalog?.scope === accountScope ? accountCatalog.accounts : [];
  const backendInstance = accountChoice?.scope === accountScope ? accountChoice.instance : null;
  const accountError = accountFailure?.scope === accountScope ? accountFailure.message : null;
  const accountsReady = accountCatalog?.scope === accountScope && !accountError;
  const implementedBackends = backends.filter((b) => b.implemented);
  const projectLabel = projectInfo?.display_name || projectInfo?.name || project;
  const backendLabel = implementedBackends.find((b) => b.id === backend)?.displayName ?? backend;
  const modelLabel = models.find((m) => m.id === model)?.name;

  // Anything the user has to fix lives under Extra settings, so open it for them.
  useEffect(() => {
    if (nodeSnapshot.error || (!nodeSnapshot.loading && !nodeReady)) setExtraOpen(true);
  }, [nodeSnapshot.error, nodeSnapshot.loading, nodeReady]);
  useEffect(() => { if (accountError) setExtraOpen(true); }, [accountError]);

  // A different source discards its selection; retrying the same source does not.
  useEffect(() => {
    setNodeChoice(null);
    setIssues([]);
    setIssue(null);
    setPrs([]);
    setPr(null);
  }, [mode, project]);

  // Issue list follows the selected project in issue mode.
  useEffect(() => {
    if (mode !== 'issue') return;
    let cancelled = false;
    setIssuesError(null);
    setIssuesLoading(true);
    gahApi
      .getChatIssues(project)
      .then(({ issues }) => { if (!cancelled) setIssues(issues); })
      .catch((err) => { if (!cancelled) setIssuesError(err instanceof Error ? err.message : String(err)); })
      .finally(() => { if (!cancelled) setIssuesLoading(false); });
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, project, sourceRetry]);

  // PR list follows the selected project in PR mode.
  useEffect(() => {
    if (mode !== 'pr') return;
    let cancelled = false;
    setPrsError(null);
    setPrsLoading(true);
    gahApi
      .getChatPrs(project)
      .then(({ prs }) => { if (!cancelled) setPrs(prs); })
      .catch((err) => { if (!cancelled) setPrsError(err instanceof Error ? err.message : String(err)); })
      .finally(() => { if (!cancelled) setPrsLoading(false); });
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, project, sourceRetry]);

  useEffect(() => {
    let cancelled = false;
    // Default to the profile's configured backend, else the first implemented one.
    gahApi
      .getManagerChatSettings()
      .then((settings) => {
        if (cancelled) return;
        const preferred = settings.profileOverrides[project] ?? settings.defaultBackend;
        setBackend(implementedBackends.some((b) => b.id === preferred) ? preferred : implementedBackends[0]?.id ?? '');
      })
      .catch(() => { if (!cancelled) setBackend(implementedBackends[0]?.id ?? ''); });
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project]);

  useEffect(() => {
    if (!project || !backend || !nodeReady || !nodeId) return;
    let cancelled = false;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 15_000);
    setAccountCatalog(null);
    setAccountFailure(null);
    gahApi.getChatAccounts(project, backend, nodeId, controller.signal)
      .then(({ accounts }) => {
        if (cancelled) return;
        const eligible = accounts.filter(instance => instance.enabled && instance.executableReady !== false && instance.authReady !== false && instance.backend === backend);
        setAccountCatalog({ scope: accountScope, accounts: eligible });
        setAccountChoice(selected => ({ scope: accountScope, instance: selected?.scope === accountScope && eligible.some(instance => instance.instance === selected.instance) ? selected.instance : null }));
      })
      .catch(() => { if (!cancelled) setAccountFailure({ scope: accountScope, message: 'Cannot load accounts on this node. Check its connection and Parasaur version, then retry.' }); })
      .finally(() => clearTimeout(timer));
    return () => { cancelled = true; clearTimeout(timer); controller.abort(); };
  }, [project, backend, nodeId, nodeReady, accountScope, accountRetry]);

  useEffect(() => {
    if (!backend || !nodeId || !nodeReady) {
      setModels([]);
      setModel(null);
      return;
    }
    let cancelled = false;
    setModels([]);
    setModel(null);
    gahApi
      .getManagerChatModelsForBackend(project, backend, nodeId || undefined, backendInstance)
      .then(({ models, currentModelId }) => {
        if (cancelled) return;
        setModels(models);
        setModel(models.length > 0 ? currentModelId ?? models[0].id : null);
      })
      .catch(() => {
        if (!cancelled) {
          setModels([]);
          setModel(null);
        }
      });
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project, backend, backendInstance, nodeId, nodeReady]);
  const startFromSource = async (source: Exclude<ChatSource, 'blank'>, number: number) => {
    if (!backend || creating || !nodeReady || !accountsReady) return;
    setCreating(true);
    setError(null);
    try {
      const { session } = source === 'issue'
        ? await gahApi.startChatFromIssue(project, number, backend, model, nodeId, backendInstance)
        : await gahApi.startChatFromPr(project, number, backend, model, nodeId, backendInstance);
      onCreated(project, session.id);
      onClose();
    } catch (err) {
      setError(creationError(err));
    } finally {
      setCreating(false);
    }
  };

  const create = async () => {
    if (!backend || !nodeReady || !accountsReady) return;
    setCreating(true);
    setError(null);
    try {
      if (mode === 'issue') {
        if (!issue) return;
        const { session } = await gahApi.startChatFromIssue(project, issue.number, backend, model, nodeId, backendInstance);
        onCreated(project, session.id);
      } else if (mode === 'pr') {
        if (!pr) return;
        const { session } = await gahApi.startChatFromPr(project, pr.number, backend, model, nodeId, backendInstance);
        onCreated(project, session.id);
      } else {
        const session = await gahApi.createChatSession(project, backend, model, title.trim() || undefined, nodeId, backendInstance);
        onCreated(project, session.id);
      }
      onClose();
    } catch (err) {
      setError(creationError(err));
    } finally {
      setCreating(false);
    }
  };

  return (
    <section aria-label="New chat" className="card w-full p-5 space-y-5">
    <div className="flex items-center justify-between">
      <h2 className="text-base font-semibold text-primary">New chat</h2>
      <button type="button" onClick={onClose} className="rounded p-1 text-muted hover:bg-overlay/5 hover:text-primary" aria-label="Close">
        <X size={16} aria-hidden="true" />
      </button>
    </div>
    {profiles.length === 0 && (
      <p className="rounded-md border border-subtle p-3 text-sm text-secondary">No project is configured yet.</p>
    )}

    {/* Mode: a blank session, grab an issue into a chat (branch for
        it, mark it in progress, seed the conversation with its body),
        or open a read-only chat seeded with a PR. */}
    <div className="flex gap-1.5" role="tablist" aria-label="Chat source">
      <button
        type="button"
        role="tab"
        aria-selected={mode === 'blank'}
        onClick={() => setMode('blank')}
        className={`flex-1 disabled:opacity-50 rounded-md px-2 py-1.5 text-xs ${mode === 'blank' ? 'bg-accent/15 border border-accent/40 text-primary' : 'border border-subtle text-secondary hover:bg-overlay/5'}`}
      >
        Blank chat
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={mode === 'issue'}
        onClick={() => setMode('issue')}
        className={`flex-1 disabled:opacity-50 rounded-md px-2 py-1.5 text-xs inline-flex items-center justify-center gap-1.5 ${mode === 'issue' ? 'bg-accent/15 border border-accent/40 text-primary' : 'border border-subtle text-secondary hover:bg-overlay/5'}`}
      >
        <CircleDot size={12} aria-hidden="true" /> From issue
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={mode === 'pr'}
        onClick={() => setMode('pr')}
        className={`flex-1 disabled:opacity-50 rounded-md px-2 py-1.5 text-xs inline-flex items-center justify-center gap-1.5 ${mode === 'pr' ? 'bg-accent/15 border border-accent/40 text-primary' : 'border border-subtle text-secondary hover:bg-overlay/5'}`}
      >
        <GitPullRequest size={12} aria-hidden="true" /> From PR
      </button>
    </div>

    {mode !== 'blank' && (
      <label className="block space-y-1 text-sm text-secondary">
        <span>Filter {mode === 'issue' ? 'issues' : 'pull requests'}</span>
        <input type="search" value={query} onChange={(event) => setQuery(event.target.value)}
          placeholder={mode === 'issue' ? 'Number, title or label' : 'Number, title, branch or author'}
          className="w-full rounded-md border border-subtle bg-raised px-2 py-1.5 text-base text-primary placeholder:text-secondary focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent" />
      </label>
    )}

    {mode === 'issue' && (
      <section className="space-y-2">
        <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-muted">
          <CircleDot size={13} aria-hidden="true" /> Issue
        </h3>
        {issuesLoading && <p className="text-xs text-muted">Loading issues…</p>}
        {issuesError && (
          <div className="space-y-2">
            <p role="alert" className="text-sm text-critical">Could not load issues. {issuesError}</p>
            <button type="button" className="btn-secondary text-xs" onClick={() => setSourceRetry((attempt) => attempt + 1)}>Retry issues</button>
          </div>
        )}
        <div className="grid gap-1 max-h-40 overflow-y-auto">
          {!issuesLoading && !issuesError && <BoundedCollection key={project} items={issues} query={query} label="issues"
            emptyMessage="No open issues for this project."
            searchText={(candidate) => `#${candidate.number} ${candidate.title} ${candidate.labels.join(' ')}`}
            isSelected={(candidate) => issue?.number === candidate.number}>
            {(candidate) => (
              <button
                key={candidate.number}
                type="button"
                onClick={() => { setIssue(candidate); void startFromSource('issue', candidate.number); }}
                disabled={creating || !backend || !nodeReady || !accountsReady}
                aria-pressed={issue?.number === candidate.number}
                className={`rounded-md px-3 py-2 text-left ${issue?.number === candidate.number ? 'bg-accent/15 border border-accent/40' : 'border border-transparent hover:bg-overlay/5'}`}
              >
                <span className="block text-sm font-medium text-primary truncate">#{candidate.number} {candidate.title}</span>
                {candidate.labels.length > 0 && (
                  <span className="block text-[11px] text-muted truncate">{candidate.labels.join(', ')}</span>
                )}
              </button>
            )}
          </BoundedCollection>}
        </div>
        {issue && (
          <p className="text-[11px] text-muted">
            Branches <span className="font-mono">gah/issue/…-{issue.number}</span>, marks #{issue.number} in progress, opens the chat seeded with the issue.
          </p>
        )}
      </section>
    )}

    {mode === 'pr' && (
      <section className="space-y-2">
        <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-muted">
          <GitPullRequest size={13} aria-hidden="true" /> Pull request
        </h3>
        {prsLoading && <p className="text-xs text-muted">Loading pull requests…</p>}
        {prsError && (
          <div className="space-y-2">
            <p role="alert" className="text-sm text-critical">Could not load pull requests. {prsError}</p>
            <button type="button" className="btn-secondary text-xs" onClick={() => setSourceRetry((attempt) => attempt + 1)}>Retry pull requests</button>
          </div>
        )}
        <div className="grid gap-1 max-h-40 overflow-y-auto">
          {!prsLoading && !prsError && <BoundedCollection key={project} items={prs} query={query} label="pull requests"
            emptyMessage="No open pull requests for this project."
            searchText={(candidate) => `#${candidate.number} ${candidate.title} ${candidate.headRefName ?? ''} ${candidate.author ?? ''}`}
            isSelected={(candidate) => pr?.number === candidate.number}>
            {(candidate) => (
              <button
                key={candidate.number}
                type="button"
                onClick={() => { setPr(candidate); void startFromSource('pr', candidate.number); }}
                disabled={creating || !backend || !nodeReady || !accountsReady}
                aria-pressed={pr?.number === candidate.number}
                className={`rounded-md px-3 py-2 text-left ${pr?.number === candidate.number ? 'bg-accent/15 border border-accent/40' : 'border border-transparent hover:bg-overlay/5'}`}
              >
                <span className="block text-sm font-medium text-primary truncate">#{candidate.number} {candidate.title}</span>
                <span className="block text-[11px] text-muted truncate">
                  {[
                    candidate.author,
                    candidate.isDraft ? 'draft' : null,
                    candidate.reviewState ? candidate.reviewState.toLowerCase().replaceAll('_', ' ') : null
                  ].filter((part) => part !== null && part.length > 0).join(' · ')}
                </span>
              </button>
            )}
          </BoundedCollection>}
        </div>
        {pr && (
          <p className="text-[11px] text-muted">
            Opens the chat seeded with PR #{pr.number} — read-only: no branch is created and the PR is not modified.
          </p>
        )}
      </section>
    )}

    <details open={extraOpen} onToggle={(event) => setExtraOpen(event.currentTarget.open)} className="group rounded-lg border border-subtle">
      <summary className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-sm text-secondary hover:text-primary [&::-webkit-details-marker]:hidden">
        <SlidersHorizontal size={14} aria-hidden="true" />
        <span className="font-medium text-primary">Extra settings</span>
        <span className="min-w-0 flex-1 truncate text-right text-xs text-muted">
          {[projectLabel, selectedNode?.displayName, [backendLabel, backendInstances.find(account => account.instance === backendInstance)?.label, modelLabel].filter(Boolean).join(' · ')].filter(Boolean).join(' / ')}
        </span>
      </summary>
      <div className="space-y-5 border-t border-subtle p-3">
    <section className="space-y-2">
      <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-muted">
        <FolderGit2 size={13} aria-hidden="true" /> Project
      </h3>
      <div className="grid gap-1">
        {profiles.map((p) => (
          <button
            key={p.name}
            type="button"
            onClick={() => setProject(p.name)}
            className={`rounded-md px-3 py-2 text-left ${project === p.name ? 'bg-accent/15 border border-accent/40' : 'border border-transparent hover:bg-overlay/5'}`}
          >
            <span className="block text-sm font-medium text-primary">{p.display_name || p.name}</span>
            <span className="block text-[11px] text-muted truncate">{p.repo}</span>
          </button>
        ))}
      </div>
    </section>

    <ChatNodePicker {...nodeSnapshot} value={nodeId} disabled={creating}
      onChange={nodeId => setNodeChoice({ project, nodeId })} />
    <p className="text-sm text-secondary">Each node uses its own checkout; files do not move.</p>

    <section className="space-y-2">
      <h3 className="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wide text-muted">
        <Cpu size={13} aria-hidden="true" /> Provider / model
      </h3>
      <div className="grid grid-cols-3 gap-1.5">
        {implementedBackends.map((b) => (
          <button
            key={b.id}
            type="button"
            onClick={() => setBackend(b.id)}
            className={`rounded-md px-2 py-2 text-sm ${backend === b.id ? 'bg-accent/15 border border-accent/40 text-primary' : 'border border-subtle text-secondary hover:bg-overlay/5'}`}
          >
            {b.displayName}
          </button>
        ))}
      </div>
      {implementedBackends.length === 0 && (
        <p role="alert" className="text-sm text-critical">No chat provider is available. Configure a provider before starting a chat.</p>
      )}
      {models.length > 0 && (
        <select
          value={model ?? ''}
          onChange={(e) => setModel(e.target.value || null)}
          className="w-full rounded-md border border-subtle bg-raised px-2 py-1.5 text-xs text-primary"
          aria-label="Model"
        >
          {models.map((m) => (
            <option key={m.id} value={m.id}>{m.name}</option>
          ))}
        </select>
      )}
      {backend && nodeReady && (
        <label className="block space-y-1 text-xs text-secondary">Account
          <select value={backendInstance ?? ''} disabled={creating || !accountsReady} onChange={(event) => setAccountChoice({ scope: accountScope, instance: event.target.value || null })} className="min-h-11 w-full rounded-md border border-subtle bg-raised px-2 py-1.5 text-base text-primary sm:min-h-0 sm:text-xs">
            <option value="">{accountsReady ? 'Default provider login' : 'Accounts not loaded'}</option>
            {backendInstances.map((instance) => (
              <option key={instance.instance} value={instance.instance}>
                {instance.label} · {instance.instance}
              </option>
            ))}
          </select>
        </label>
      )}
      {backend && nodeReady && !accountsReady && !accountError && <p role="status" className="text-xs text-secondary">Loading accounts on {selectedNode?.displayName}…</p>}
      {accountError && <div className="space-y-2"><p role="alert" className="text-sm text-critical">{accountError}</p><button type="button" className="btn-secondary text-xs max-sm:min-h-11" onClick={() => setAccountRetry(value => value + 1)}>Retry accounts</button></div>}
      {backend && models.length === 0 && (
        <p className="text-[11px] text-muted">No model selected. Use the provider's configured default.</p>
      )}
    </section>
      </div>
    </details>

    {mode === 'blank' && (
      <section className="space-y-2">
        <label htmlFor="new-chat-title" className="block text-xs font-medium text-secondary">Chat name</label>
        <input
          id="new-chat-title"
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="e.g. Fix the retry loop"
          required
          className="w-full rounded-md border border-subtle bg-raised px-2 py-1.5 text-base text-primary"
        />
      </section>
    )}

    {error && <p className="text-xs text-critical">{error}</p>}

    <div className="flex items-center justify-end gap-2 pt-1">
      <button type="button" onClick={onClose} className="btn-secondary text-xs">Cancel</button>
      <button
        type="button"
        onClick={create}
        disabled={creating || !backend || !nodeReady || !accountsReady || profiles.length === 0 || (mode === 'blank' && !title.trim()) || (mode === 'issue' && !issue) || (mode === 'pr' && !pr)}
        className="btn-primary text-xs"
      >
        {creating ? 'Creating…' : 'Start chat'}
      </button>
    </div>
    </section>
  );
}
