/**
 * Runtime validation of the GitLab dispatch context (#1595).
 *
 * The `gitlab` metadata arrives over the network (POST /api/dispatch and the
 * `session.start` WebSocket message), so both paths share this one runtime
 * validator: the contracts type it as optional, but a type never validates
 * network JSON. An invalid context is rejected before anything starts — a
 * malformed context must not silently start an unscoped run — and the same
 * project-path rule backs the runs feed's query parsing.
 */
import type { GitLabDispatchContext } from '@git-agent-harness/contracts';

/** One path segment of a GitLab group or project path: GitLab allows
 * letters, digits, underscores, dots and dashes, and rejects a leading
 * dot or dash (which also rules out `.`/`..` traversal fragments). */
const GITLAB_PATH_SEGMENT = /^[A-Za-z0-9_][A-Za-z0-9_.-]*$/;

/** A full GitLab project path, e.g. `group/project`: non-empty
 * `/`-separated segments of GitLab-allowed characters. */
export function isGitLabProjectPath(project: string): boolean {
  return project.length > 0 && project.length <= 512
    && !/[\x00-\x1f\x7f]/.test(project)
    && project.split('/').every(segment => GITLAB_PATH_SEGMENT.test(segment));
}

/** An http(s) URL with a host — the only web origins the panel can prove. */
export function isHttpWebUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    return (parsed.protocol === 'http:' || parsed.protocol === 'https:') && parsed.hostname !== '';
  } catch {
    return false;
  }
}

/** Valid GitLab page context for a dispatch started against a GitLab
 * project, or null when absent. Invalid context is 'invalid': the caller
 * rejects it (HTTP 400 / a WebSocket error) before starting anything. */
export function parseGitLabDispatchContext(value: unknown): GitLabDispatchContext | null | 'invalid' {
  if (value === undefined || value === null) return null;
  if (typeof value !== 'object' || Array.isArray(value)) return 'invalid';
  const record = value as Record<string, unknown>;
  const project = typeof record.project === 'string' ? record.project.trim() : '';
  const webUrl = typeof record.webUrl === 'string' ? record.webUrl.trim() : '';
  const iid = typeof record.iid === 'number' && Number.isSafeInteger(record.iid) && record.iid > 0 ? record.iid : null;
  if (!isGitLabProjectPath(project)
    || (record.kind !== 'issue' && record.kind !== 'merge_request')
    || iid === null
    || !isHttpWebUrl(webUrl)) return 'invalid';
  return { project, kind: record.kind, iid, webUrl };
}
