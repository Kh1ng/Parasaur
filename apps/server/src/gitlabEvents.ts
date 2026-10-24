import { createHash, timingSafeEqual } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { Router } from 'express';
import type { SessionOptions } from './sessions/SessionManager.js';
import { runConfigShowProfile } from './gahCli.js';
import { writeFileAtomic } from './atomicFile.js';

export type GitlabHookProject = {
  projectId: number;
  profile: string;
  repo: string;
  apiBase: string;
  bot: string;
  providerKind: SessionOptions['providerKind'];
  instanceId: SessionOptions['instanceId'];
};
type Task = { kind: 'issues' | 'merge_requests'; iid: number; instruction?: string };
type ObjectValue = Record<string, any>;
const object = (value: unknown): ObjectValue => value !== null && typeof value === 'object' && !Array.isArray(value) ? value : {};
const positive = (value: unknown): value is number => Number.isSafeInteger(value) && Number(value) > 0;
const digest = (value: string) => createHash('sha256').update(value).digest();

export function hookTask(payload: unknown, project: GitlabHookProject): Task | null {
  const p = object(payload);
  const a = object(p.object_attributes);
  const kind = p.object_kind ?? p.event_type;
  if (kind === 'issue' && positive(a.iid) && ['open', 'update'].includes(a.action) && a.state === 'opened') {
    const assigned = Array.isArray(p.assignees) && p.assignees.some((u: unknown) => object(u).username === project.bot);
    const changed = object(object(p.changes).assignees);
    const previous = Array.isArray(changed.previous) ? changed.previous : [];
    if (assigned && (a.action === 'open' || (Array.isArray(changed.current) && !previous.some((u: unknown) => object(u).username === project.bot)))) return { kind: 'issues', iid: a.iid };
  }
  if (kind === 'merge_request' && positive(a.iid) && a.state === 'opened') {
    const changes = object(object(p.changes).reviewers);
    const requested = Array.isArray(p.reviewers) && p.reviewers.some((u: unknown) => object(u).username === project.bot && (a.action === 'open' || object(u).re_requested === true));
    if (requested) return { kind: 'merge_requests', iid: a.iid };
    if (Array.isArray(changes.current) && changes.current.some((u: unknown) => object(u).username === project.bot) &&
      !(Array.isArray(changes.previous) && changes.previous.some((u: unknown) => object(u).username === project.bot))) return { kind: 'merge_requests', iid: a.iid };
  }
  if (kind === 'note' && (!a.action || a.action === 'create') && typeof a.note === 'string' && /(^|[^\w@])@parasaur(?![\w.-])/i.test(a.note)) {
    const target = a.noteable_type === 'Issue' ? object(p.issue) : a.noteable_type === 'MergeRequest' ? object(p.merge_request) : {};
    if (positive(target.iid)) return { kind: a.noteable_type === 'Issue' ? 'issues' : 'merge_requests', iid: target.iid, instruction: a.note };
  }
  return null;
}

export function gitlabEventsRouter(deps: {
  startSession: (options: SessionOptions) => Promise<{ id: string }>;
  projects?: GitlabHookProject[];
  secret?: string;
  stateDir?: string;
  panelUrl?: string;
  authorize?: (project: GitlabHookProject, payload: ObjectValue) => Promise<boolean>;
  comment?: (project: GitlabHookProject, task: Task, text: string) => Promise<void>;
}) {
  const router = Router();
  router.post('/api/integrations/gitlab/events', (req, res) => {
    const secret = deps.secret ?? process.env.GAH_GITLAB_HOOK_SECRET;
    const supplied = req.get('X-Parasaur-Hook-Secret');
    if (!secret || !supplied || !timingSafeEqual(digest(secret), digest(supplied))) return void res.status(401).json({ error: 'authentication_required' });
    let projects: GitlabHookProject[];
    try { projects = deps.projects ?? JSON.parse(process.env.GAH_GITLAB_HOOK_PROJECTS ?? '[]'); }
    catch { return void res.status(503).json({ error: 'hook_configuration_invalid' }); }
    if (!Array.isArray(projects) || projects.some(p => !p || !positive(p.projectId) ||
      !['profile', 'repo', 'apiBase', 'bot', 'providerKind', 'instanceId'].every(key => typeof object(p)[key] === 'string' && object(p)[key].length > 0))) {
      return void res.status(503).json({ error: 'hook_configuration_invalid' });
    }
    const payload = object(req.body);
    const project = projects.find(p => p.projectId === object(payload.project).id);
    const task = project && hookTask(payload, project);
    if (!project || !task) return void res.status(200).json({ ignored: true });
    const eventId = digest(JSON.stringify([project.projectId, req.get('X-Gitlab-Event-UUID') ?? payload])).toString('hex');
    const directory = deps.stateDir ?? join(homedir(), '.local/share/gah/sessions/gitlab-events');
    const receipt = join(directory, `${eventId}.json`);
    const context = { profile: project.profile, projectId: project.projectId, task };
    try {
      mkdirSync(directory, { recursive: true, mode: 0o700 });
      writeFileSync(receipt, JSON.stringify({ ...context, status: 'accepted' }), { flag: 'wx', mode: 0o600 });
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code === 'EEXIST') return void res.status(200).json({ duplicate: true });
      return void res.status(503).json({ error: 'hook_storage_unavailable' });
    }
    res.status(202).json({ accepted: true });
    setImmediate(() => { void (async () => {
      let sessionId: string | undefined;
      try {
        const authorized = deps.authorize ? await deps.authorize(project, payload) : await authorize(project, payload);
        if (!authorized) {
          writeFileAtomic(receipt, JSON.stringify({ ...context, status: 'ignored_untrusted_author' }));
          return;
        }
        const session = await deps.startSession({ profile: project.profile, repo: project.repo, providerKind: project.providerKind,
          instanceId: project.instanceId, mode: task.kind === 'merge_requests' && !task.instruction ? 'review' : 'fix',
          mr: task.kind === 'merge_requests' ? String(task.iid) : undefined,
          target: task.instruction ? `GitLab ${task.kind} #${task.iid}\nInstruction from note:\n${task.instruction}` : `#${task.iid}`,
          requestId: `gitlab-${eventId}` });
        sessionId = session.id;
        writeFileAtomic(receipt, JSON.stringify({ ...context, status: 'queued', sessionId }));
        const panel = new URL(deps.panelUrl ?? process.env.GAH_PANEL_URL ?? 'http://localhost:3773');
        panel.searchParams.set('session', session.id);
        panel.searchParams.set('profile', project.profile);
        const text = `Parasaur run queued: ${panel.href}`;
        if (deps.comment) await deps.comment(project, task, text);
        else {
          const token = process.env.GITLAB_TOKEN;
          if (!token) throw new Error('GitLab token unavailable');
          const response = await fetch(`${project.apiBase.replace(/\/$/, '')}/projects/${project.projectId}/${task.kind}/${task.iid}/notes`, {
            method: 'POST', headers: { 'PRIVATE-TOKEN': token, 'Content-Type': 'application/json' }, body: JSON.stringify({ body: text }), signal: AbortSignal.timeout(10_000), redirect: 'error'
          });
          if (!response.ok) throw new Error('GitLab comment failed');
        }
      } catch {
        // Keep the receipt: an uncertain dispatch must never be started twice.
        writeFileAtomic(receipt, JSON.stringify({ ...context, status: sessionId ? 'queued_comment_failed' : 'needs_review', sessionId }));
        console.error('[gitlab-hook] Event requires review:', eventId);
      }
    })().catch(() => console.error('[gitlab-hook] Could not update event receipt:', eventId)); });
  });
  return router;
}

export async function authorize(project: GitlabHookProject, payload: ObjectValue, deps = {
  config: runConfigShowProfile,
  user: async (id: number): Promise<unknown> => {
    const response = await fetch(`${project.apiBase.replace(/\/$/, '')}/users/${id}`, {
      headers: { 'PRIVATE-TOKEN': process.env.GITLAB_TOKEN ?? '' }, redirect: 'error', signal: AbortSignal.timeout(10_000)
    });
    return response.ok ? response.json() : null;
  }
}): Promise<boolean> {
  const user = object(payload.user);
  if (!positive(user.id)) return false;
  const config = await deps.config(project.profile);
  const trusted = async (id: number, username?: string) => {
    const actor = object(await deps.user(id));
    if (actor.id !== id || (username !== undefined && actor.username !== username)) return false;
    const authors = actor.bot === true ? config.trusted_issue_bot_authors : actor.bot === false ? config.trusted_issue_human_authors : null;
    return typeof actor.username === 'string' && !!authors?.some(name => name.toLowerCase() === actor.username.toLowerCase());
  };
  if (!await trusted(user.id, user.username)) return false;
  if ((payload.object_kind ?? payload.event_type) === 'issue') {
    const author = object(payload.object_attributes).author_id;
    if (!positive(author)) return false;
    if (author !== user.id && !await trusted(author)) return false;
  }
  return true;
}
