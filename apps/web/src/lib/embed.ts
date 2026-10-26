/**
 * GitLab panel embed contract (#1593), client side. The parasaur-gitlab side
 * panel opens `<parasaurUrl>embed?gitlab=&url=&project=&kind=&iid=&ref=&path=`
 * and talks to the embed view over postMessage; the shapes and rules here
 * follow docs/parasaur-contract.md in the companion repo:
 *
 * - page context arrives from the query string and/or a `parasaur:context`
 *   message when the GitLab page changes or the panel reopens;
 * - the embed view sends `parasaur:ready` once loaded (the panel replies
 *   with the context), `parasaur:close` to close itself, and
 *   `parasaur:navigate` to move GitLab to a same-origin path;
 * - inbound messages count only when `event.origin` is one of the origins
 *   the server configured through `GAH_GITLAB_ORIGIN`; everything else is
 *   ignored, and outbound messages are addressed to those origins only,
 *   never `*`.
 */

import { readBootstrap } from './basePath.js';

export const EMBED_CONTEXT_KINDS = ['issue', 'merge_request', 'file', 'tree', 'pipeline', 'job'] as const;
export type EmbedContextKind = (typeof EMBED_CONTEXT_KINDS)[number];

/** Human labels for the context kinds, shared by the embed surface and the
 * chat context block. */
export const EMBED_CONTEXT_KIND_LABELS: Record<EmbedContextKind, string> = {
  issue: 'Issue',
  merge_request: 'Merge request',
  file: 'File',
  tree: 'Tree',
  pipeline: 'Pipeline',
  job: 'Job'
};

/** The GitLab page the panel is looking at. */
export interface EmbedContext {
  /** Page origin, e.g. https://gitlab.example.com. */
  gitlab: string;
  /** Page URL. */
  url: string;
  /** Project full path (group/project), when known. */
  project: string | null;
  kind: EmbedContextKind | null;
  /** Issue or merge request iid, when known. */
  iid: number | null;
  /** File/tree ref and path, when known. */
  ref: string | null;
  path: string | null;
}

/** The context fields the panel sends; bounded so a crafted query string or
 * message cannot balloon state or smuggle control characters. */
const EMBED_CONTEXT_FIELDS = ['gitlab', 'url', 'project', 'kind', 'iid', 'ref', 'path'] as const;
const CONTEXT_FIELD_LIMIT = 512;
const CONTEXT_URL_LIMIT = 2048;

function boundedString(value: unknown, limit: number): string | null {
  if (typeof value !== 'string' || value.length === 0 || value.length > limit) return null;
  if (value.trim() === '' || /[\x00-\x1f\x7f]/.test(value)) return null;
  return value;
}

function httpOrigin(value: string): string | null {
  try {
    const url = new URL(value);
    // Exactly http/https (startsWith('http') would admit 'httpx://', whose
    // origin serializes as 'null'), and never a wildcard host.
    if ((url.protocol !== 'http:' && url.protocol !== 'https:') || url.username || url.password
      || url.pathname !== '/' || url.search !== '' || url.hash !== '' || url.hostname.includes('*')) return null;
    return url.origin;
  } catch {
    return null;
  }
}

function httpUrl(value: string): string | null {
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
  } catch {
    return null;
  }
}

/** Validate the page context from either the query string or a panel
 * message. Unknown fields are ignored; a missing or invalid GitLab origin
 * or page URL makes the whole context unusable, so it is rejected. */
export function readEmbedContext(fields: { gitlab?: unknown; url?: unknown; project?: unknown; kind?: unknown; iid?: unknown; ref?: unknown; path?: unknown } = {}): EmbedContext | null {
  const gitlabRaw = boundedString(fields.gitlab, CONTEXT_FIELD_LIMIT);
  const urlRaw = boundedString(fields.url, CONTEXT_URL_LIMIT);
  if (gitlabRaw === null || urlRaw === null) return null;
  const gitlab = httpOrigin(gitlabRaw);
  const url = httpUrl(urlRaw);
  if (gitlab === null || url === null || new URL(url).origin !== gitlab) return null;
  const kind = typeof fields.kind === 'string' && (EMBED_CONTEXT_KINDS as readonly string[]).includes(fields.kind)
    ? fields.kind as EmbedContextKind
    : null;
  const iidText = typeof fields.iid === 'number' ? String(fields.iid) : boundedString(fields.iid, 32);
  const iid = iidText !== null && /^\d{1,10}$/.test(iidText) && !iidText.startsWith('0') ? Number(iidText) : null;
  const project = fields.project === undefined ? null : boundedString(fields.project, CONTEXT_FIELD_LIMIT);
  return {
    gitlab,
    url,
    project: project !== null && !project.startsWith('/') ? project : null,
    kind,
    iid,
    ref: fields.ref === undefined ? null : boundedString(fields.ref, CONTEXT_FIELD_LIMIT),
    path: fields.path === undefined ? null : boundedString(fields.path, CONTEXT_URL_LIMIT)
  };
}

/** The page context from the embed view's query string. */
export function parseEmbedContext(search: string | URLSearchParams): EmbedContext | null {
  const params = search instanceof URLSearchParams ? search : new URLSearchParams(search);
  return readEmbedContext(Object.fromEntries(EMBED_CONTEXT_FIELDS.map(field => [field, params.get(field) ?? undefined])));
}

/** Serialize a context back into the query string the standalone view and
 * the panel's pop-out link share. */
export function embedContextParams(context: EmbedContext): URLSearchParams {
  const params = new URLSearchParams();
  params.set('gitlab', context.gitlab);
  params.set('url', context.url);
  if (context.project !== null) params.set('project', context.project);
  if (context.kind !== null) params.set('kind', context.kind);
  if (context.iid !== null) params.set('iid', String(context.iid));
  if (context.ref !== null) params.set('ref', context.ref);
  if (context.path !== null) params.set('path', context.path);
  return params;
}

/** The context a standalone link (`/?gitlab=…`) carries. Read once at
 * startup, like takeActivityDeepLink: the parameters are removed from the
 * URL so later navigation restores the dashboard's own links, and repeat
 * calls in the same page load return the same context. */
let standaloneContext: EmbedContext | null | undefined;

export function takeStandaloneEmbedContext(search = window.location.search): EmbedContext | null {
  if (standaloneContext !== undefined) return standaloneContext;
  standaloneContext = parseEmbedContext(search);
  if (standaloneContext !== null && typeof window !== 'undefined') {
    const url = new URL(window.location.href);
    for (const field of EMBED_CONTEXT_FIELDS) url.searchParams.delete(field);
    window.history.replaceState(window.history.state, '', url);
  }
  return standaloneContext;
}

// ---- postMessage API -------------------------------------------------------

/** Inbound: only `parasaur:context` is defined panel-to-Parasaur. */
export type EmbedInboundMessage = { type: 'parasaur:context'; context: EmbedContext };

/** Outbound, Parasaur-to-panel. */
export type EmbedOutboundMessage =
  | { type: 'parasaur:ready' }
  | { type: 'parasaur:close' }
  | { type: 'parasaur:navigate'; path: string };

/** Origins allowed to message the embed view: the ones the server
 * configured through `GAH_GITLAB_ORIGIN` and injected with the document. */
export function embedAllowedOrigins(): string[] {
  return readBootstrap().gitLabOrigins;
}

/** `event.origin` must match a configured origin exactly. */
export function embedMessageOriginAllowed(origin: string, allowed: string[]): boolean {
  return allowed.includes(origin);
}

/** Interpret one MessageEvent from the panel. Every message type from the
 * contract appears in the switch: the inbound one is parsed, the three
 * outbound ones are ignored when received, and anything else (unknown
 * types, malformed payloads, disallowed origins) is dropped. */
export function handleEmbedMessage(event: { origin: string; data: unknown }, allowed: string[]): EmbedInboundMessage | null {
  if (!embedMessageOriginAllowed(event.origin, allowed)) return null;
  const data = event.data;
  if (data === null || typeof data !== 'object') return null;
  const type = (data as { type?: unknown }).type;
  switch (type) {
    case 'parasaur:context': {
      const raw = (data as { context?: unknown }).context;
      const context = readEmbedContext(raw && typeof raw === 'object' ? raw as Record<string, unknown> : {});
      return context === null ? null : { type: 'parasaur:context', context };
    }
    // Outbound-only messages from the contract: receiving one is not an error,
    // but it is never trusted as an instruction.
    case 'parasaur:ready':
    case 'parasaur:close':
    case 'parasaur:navigate':
      return null;
    default:
      return null;
  }
}

/** A same-origin path the panel may navigate GitLab to: absolute, not
 * protocol-relative, bounded, printable. */
export function embedNavigatePath(value: unknown): string | null {
  if (typeof value !== 'string' || value.length === 0 || value.length > 2048) return null;
  if (!value.startsWith('/') || value.startsWith('//') || value.startsWith('/\\')) return null;
  if (/[\x00-\x1f\x7f]/.test(value)) return null;
  return value;
}

/** Post a contract message to the embedding panel, addressed to the
 * configured origins only (never `*`). No-op when not framed. */
export function postEmbedMessage(message: EmbedOutboundMessage, targetOrigins: string[]): void {
  if (typeof window === 'undefined' || window.parent === window) return;
  for (const target of targetOrigins) {
    try {
      window.parent.postMessage(message, target);
    } catch {
      // An unreachable target drops the message; the panel re-syncs on ready.
    }
  }
}

// ---- project matching ------------------------------------------------------

/** The context block an ordinary embed-view chat sends with its first turn
 * (and again when the page context changes): the same resolved
 * project/kind/iid/ref/path the seeded issue and MR chats carry, written as
 * the opening block of the outgoing message. */
export function embedContextPreamble(context: EmbedContext): string {
  const lines = ['GitLab page context (from the Parasaur panel):'];
  if (context.project !== null) lines.push(`Project: ${context.project}`);
  if (context.kind !== null && context.iid !== null) lines.push(`${EMBED_CONTEXT_KIND_LABELS[context.kind]}: #${context.iid}`);
  if (context.ref !== null) lines.push(`Ref: ${context.ref}`);
  if (context.path !== null) lines.push(`Path: ${context.path}`);
  lines.push(`Page: ${context.url}`);
  return lines.join('\n');
}

/** Whether this context still needs to ride along with a send: it does
 * until the same context signature has been delivered to the conversation. */
export function embedContextSignature(context: EmbedContext): string {
  return JSON.stringify([context.project, context.kind, context.iid, context.ref, context.path, context.url]);
}

/** A chat profile's provider identity, as the embed context needs it. */
export interface EmbedProjectLike {
  name: string;
  provider: string;
  repo: string;
  web_url: string | null;
}

function repoWebUrl(profile: EmbedProjectLike): string | null {
  if (profile.web_url === null) return null;
  try {
    const url = new URL(profile.web_url);
    return `${url.origin}${url.pathname.replace(/\/+$/, '')}`;
  } catch {
    return null;
  }
}

/** The Parasaur project for a GitLab page context: the profile whose
 * provider link is exactly `<gitlab origin>/<project>`, or the unique
 * `gitlab` provider profile with that repository path. Ambiguous or absent
 * matches return null, mirroring the event dispatcher's fail-closed rule. */
export function matchEmbedProfile<T extends EmbedProjectLike>(profiles: T[], context: EmbedContext): T | null {
  if (context.project === null) return null;
  const byUrl = profiles.filter(profile => repoWebUrl(profile) === `${context.gitlab}/${context.project}`);
  if (byUrl.length === 1) return byUrl[0];
  const byRepo = profiles.filter(profile => profile.provider === 'gitlab' && profile.repo === context.project);
  if (byUrl.length === 0 && byRepo.length === 1) return byRepo[0];
  return null;
}
