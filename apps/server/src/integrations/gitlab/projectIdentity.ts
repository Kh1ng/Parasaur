import { homedir } from 'node:os';
import type { ProfileSummary } from '@git-agent-harness/contracts';
import { execProviderCli } from '../../managerChat/providerCli.js';
import { record } from './classifier.js';

/** Provider reads always use the profile's configured transport. */
export async function readGitLab(profile: ProfileSummary, path: string, cli = execProviderCli): Promise<unknown> {
  const host = new URL(profile.provider_api_base || profile.web_url!).host;
  const { stdout } = await cli('glab', ['api', path, '--hostname', host, '--method', 'GET'], profile.local_path || homedir());
  return JSON.parse(stdout || 'null') as unknown;
}

/** Resolve a reverse proxy's public project URL using GitLab's own project record.
 * Exact configured URLs work with older profiles. An alias also needs a pinned
 * numeric project ID, matching namespace and the complete published HTTPS URL.
 */
export async function publicProjectProfiles(
  profiles: ProfileSummary[], repo: string, url: string, cli = execProviderCli, eventProjectId?: number
): Promise<ProfileSummary[]> {
  const matches: ProfileSummary[] = [];
  for (const profile of profiles) {
    if (profile.provider !== 'gitlab' || profile.repo !== repo || !profile.web_url) continue;
    const pinnedId = profile.provider_project_id;
    const numericId = pinnedId && /^[1-9]\d*$/.test(pinnedId);
    if (numericId && eventProjectId !== undefined && String(eventProjectId) !== pinnedId) continue;
    if (profile.web_url.replace(/\/$/, '') === url) {
      matches.push(profile);
      continue;
    }
    if (!url.startsWith('https://') || !numericId) continue;
    let project: Record<string, unknown>;
    try {
      project = record(await readGitLab(profile, `projects/${pinnedId}`, cli));
    } catch {
      // The runs endpoint can display this error. Provider output is private.
      throw new Error('Could not verify the public GitLab project URL');
    }
    if (Number.isSafeInteger(project.id) && String(project.id) === pinnedId
      && project.path_with_namespace === repo && project.web_url === url) matches.push(profile);
  }
  return matches;
}
