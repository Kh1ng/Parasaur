import { createHash } from 'node:crypto';

export function record(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function positiveId(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0;
}

export type GitLabEvent = {
  key: string;
  project: { id: number; repo: string; url: string };
  actorId: number;
} & (
  | { kind: 'issue'; iid: number }
  | { kind: 'merge_request'; iid: number }
  | { kind: 'note'; iid: number; resource: 'issues' | 'merge_requests'; instruction: string }
  | { kind: 'pipeline'; branch: string; sha: string; pipelineId: number }
);

function botIn(value: unknown, bot: string): Record<string, unknown> | undefined {
  return Array.isArray(value) ? value.map(record).find(user => typeof user.username === 'string' && user.username.toLowerCase() === bot.toLowerCase()) : undefined;
}

/** Detect additions and re-requests, never unrelated edits while the bot remains assigned. */
function requested(users: unknown, change: unknown, action: unknown, bot: string): boolean {
  const currentBot = botIn(users, bot);
  const delta = record(change);
  const previous = Array.isArray(change) ? change[0] : delta.previous;
  const current = Array.isArray(change) ? change[1] : delta.current;
  if (action === 'open') return currentBot !== undefined;
  if (action !== 'update' || !Array.isArray(previous) || !Array.isArray(current)) return false;
  const added = botIn(current, bot);
  if (added && !botIn(previous, bot)) return true;
  if (added?.re_requested === true && botIn(previous, bot)?.re_requested !== true) return true;
  return currentBot !== undefined && positiveId(currentBot.id)
    && current.includes(currentBot.id) && !previous.includes(currentBot.id);
}

/** Validate hook data and classify the four supported kinds. Other events cost no provider calls. */
export function classifyEvent(body: unknown, bot: string, pipelineEnabled: boolean, eventUuid?: string): GitLabEvent | null {
  const event = record(body);
  const attrs = record(event.object_attributes);
  const project = record(event.project);
  const actor = record(event.user);
  const changes = record(event.changes);
  if (!positiveId(project.id) || typeof project.path_with_namespace !== 'string' || typeof project.web_url !== 'string'
    || !positiveId(actor.id) || !positiveId(attrs.id)) return null;
  let projectUrl: URL;
  try { projectUrl = new URL(project.web_url); } catch { return null; }
  if (projectUrl.protocol !== 'https:' || projectUrl.username || projectUrl.password || projectUrl.search || projectUrl.hash
    || projectUrl.pathname !== `/${project.path_with_namespace}`) return null;
  const base = { project: { id: project.id, repo: project.path_with_namespace, url: projectUrl.href.replace(/\/$/, '') }, actorId: actor.id };
  const kind = event.object_kind;
  const action = attrs.action ?? (kind === 'note' ? 'create' : attrs.status);
  // The file hook sends no UUID header. Include the revision for repeated assignments and edits.
  const uuid = eventUuid ?? event.event_uuid;
  const identity = typeof uuid === 'string' && uuid.length > 0 && uuid.length <= 256 ? uuid : null;
  const key = createHash('sha256').update(JSON.stringify([
    base.project.url, kind, identity ?? attrs.id, identity ? null : action,
    identity || kind === 'pipeline' ? null : attrs.updated_at,
    !identity && kind === 'note' ? attrs.note : null,
    !identity && kind === 'issue' ? changes.assignees ?? changes.assignee_ids : null,
    !identity && kind === 'merge_request' ? changes.reviewers ?? changes.reviewer_ids : null,
  ])).digest('hex');
  const common = { ...base, key };
  if (kind !== 'issue' && kind !== 'merge_request' && kind !== 'note' && kind !== 'pipeline') return null;
  switch (kind) {
    case 'issue':
      return positiveId(attrs.iid) && attrs.state === 'opened'
        && requested(event.assignees, changes.assignees ?? changes.assignee_ids, action, bot)
        ? { ...common, kind, iid: attrs.iid } : null;
    case 'merge_request':
      return positiveId(attrs.iid) && attrs.state === 'opened'
        && requested(event.reviewers, changes.reviewers ?? changes.reviewer_ids, action, bot)
        ? { ...common, kind, iid: attrs.iid } : null;
    case 'note': {
      if (attrs.system === true || !['create', 'update'].includes(String(action)) || typeof attrs.note !== 'string'
        || attrs.note.length > 32_768 || !/(^|[^\w@])@parasaur(?![\w-])/i.test(attrs.note)) return null;
      const resource = attrs.noteable_type === 'Issue' ? 'issues' : attrs.noteable_type === 'MergeRequest' ? 'merge_requests' : null;
      const target = record(resource === 'issues' ? event.issue : event.merge_request);
      return resource && positiveId(target.iid) ? { ...common, kind, resource, iid: target.iid, instruction: attrs.note } : null;
    }
    case 'pipeline':
      return pipelineEnabled && attrs.status === 'failed' && attrs.tag === false
        && typeof attrs.ref === 'string' && attrs.ref.length <= 255 && !/[\x00-\x20\x7f]/.test(attrs.ref)
        && typeof attrs.sha === 'string' && /^[a-f0-9]{40,64}$/.test(attrs.sha)
        ? { ...common, kind, branch: attrs.ref, sha: attrs.sha, pipelineId: attrs.id } : null;
    default: {
      const exhaustive: never = kind;
      return exhaustive;
    }
  }
}
