/**
 * Release channel feed (issue #1416). Central compares its own version
 * against the channel's latest published release so the dashboard can show
 * "Update available · vX → vY" without anyone running `gah update` by hand.
 *
 * The channel is the prerelease "edge" feed by default (a green merge to
 * main publishes it; no tag needed) or the stable tag feed via
 * PARASAUR_RELEASE_CHANNEL=stable. Results are cached for a few minutes: the
 * dashboard polls this on load, and unbounded GitHub API reads would burn
 * the anonymous rate limit for nothing.
 */
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { writeFileAtomic } from './atomicFile.js';
import { buildCommit as commit, runningBuild } from './buildRevision.js';
import { readFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { promisify } from 'node:util';
import {
  COORDINATOR_VERSION,
  compareVersions,
  type ReleaseChannel,
  type ReleaseChannelStatus
} from '@git-agent-harness/contracts';

const exec = promisify(execFile);

const configuredRepo = () => process.env.PARASAUR_RELEASE_REPO || 'Kh1ng/Parasaur';
const configuredChannel = (): ReleaseChannel => process.env.PARASAUR_RELEASE_CHANNEL === 'stable' ? 'stable' : 'edge';
const CACHE_TTL_MS = 5 * 60_000;
const MAX_NOTES_CHARS = 8_000;
const MANIFEST_ASSET_NAME = 'edge-manifest.json';

interface GithubRelease {
  tag_name: string;
  draft: boolean;
  prerelease: boolean;
  html_url: string;
  published_at: string | null;
  body: string | null;
  assets: { id?: number; name: string; browser_download_url: string; state: string }[];
}

export function serverReleaseManifest(): string {
  const releasePath = configuredChannel() === 'stable' ? 'latest/download' : 'download/edge';
  return process.env.PARASAUR_RELEASE_MANIFEST ||
    `https://github.com/${configuredRepo()}/releases/${releasePath}/${MANIFEST_ASSET_NAME}`;
}

export interface ReleaseFeedOptions {
  fetch?: typeof fetch;
  now?: () => number;
  /** Overrides PARASAUR_RELEASE_REPO for tests. */
  repo?: string;
  /** Overrides PARASAUR_RELEASE_CHANNEL for tests. */
  channel?: ReleaseChannel;
  currentCommit?: string | null;
  manifest?: string;
}

let cache: { key: string; at: number; status: ReleaseChannelStatus } | null = null;

export function resetReleaseFeedCache(): void {
  cache = null;
}

/** Best-effort token: env first, then `gh auth token`; anonymous reads are
 * fine (60/hour against a 5-minute cache) so absence is not an error. */
async function githubToken(): Promise<string | null> {
  const fromEnv = process.env.GH_TOKEN || process.env.GITHUB_TOKEN;
  if (fromEnv) return fromEnv;
  try {
    const { stdout } = await exec('gh', ['auth', 'token'], { timeout: 5_000, maxBuffer: 4096 });
    const token = stdout.trim();
    return token || null;
  } catch {
    return null;
  }
}

async function githubHeaders(): Promise<Record<string, string>> {
  const headers: Record<string, string> = {
    Accept: 'application/vnd.github+json',
    'X-GitHub-Api-Version': '2022-11-28'
  };
  const token = await githubToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  return headers;
}

async function latestChannelRelease(
  fetchFn: typeof fetch, repo: string, channel: ReleaseChannel, headers: Record<string, string>
): Promise<GithubRelease | null> {
  const ref = channel === 'edge' ? 'tags/edge' : 'latest';
  const response = await fetchFn(`https://api.github.com/repos/${repo}/releases/${ref}`, {
    headers,
    signal: AbortSignal.timeout(15_000)
  });
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(`Cannot read releases for ${repo} (HTTP ${response.status}).`);
  const release = await response.json() as GithubRelease;
  if (release.draft || (channel === 'edge' ? release.tag_name !== 'edge' : release.prerelease)) return null;
  return release;
}

interface ReleaseDocument {
  schema: number;
  channel: string;
  version: string;
  commit?: string;
  assets: { name: string; kind: string; target?: string; sha256?: string; url?: string }[];
}

function hasServerKit(data: ReleaseDocument): boolean {
  if (!Array.isArray(data.assets)) return false;
  const platform = process.platform === 'darwin' ? 'macos' : process.arch === 'arm64' ? 'linux-aarch64' : 'linux-x86_64';
  const assets = data.assets.filter(asset =>
    typeof asset?.name === 'string' && typeof asset.sha256 === 'string' && /^[a-f0-9]{64}$/i.test(asset.sha256)
  );
  const binaries = assets.filter(asset =>
    asset.name.includes(platform) || (typeof asset.target === 'string' && asset.target.includes(platform))
  );
  return assets.some(asset => asset.kind === 'server-bundle') &&
    binaries.some(asset => asset.kind === 'cli') && binaries.some(asset => asset.kind === 'mcp-server');
}

async function readManifest(source: string, fetchFn: typeof fetch, headers?: Record<string, string>): Promise<ReleaseDocument> {
  let contents: string;
  if (/^https?:\/\//i.test(source)) {
    const response = await fetchFn(source, { headers, signal: AbortSignal.timeout(10_000) });
    if (!response.ok) throw new Error(`Release manifest unavailable (HTTP ${response.status}).`);
    contents = await response.text();
  } else {
    contents = await readFile(source, 'utf8');
  }
  const data: Partial<ReleaseDocument> | null = JSON.parse(contents);
  if (data?.schema !== 1 || typeof data.version !== 'string' || !/^v?\d+(?:\.\d+){1,2}(?:[-+][\w.+-]+)?$/.test(data.version)) {
    throw new Error('The release manifest has no supported product version.');
  }
  if (typeof data.channel !== 'string') throw new Error('The release manifest has no channel.');
  return data as ReleaseDocument;
}

export class ReleaseChanged extends Error {}

export function persistReleaseManifest(statePath: string, contents: string): string {
  const digest = createHash('sha256').update(contents).digest('hex');
  const path = statePath + '.manifest-' + digest + '.json';
  writeFileAtomic(path, contents);
  return path;
}

/** Preserve the selected manifest across the dashboard-to-CLI handoff. Resolve
 * its relative asset URLs before moving the snapshot into the state directory. */
export async function selectReleaseManifest(expected: string, source = serverReleaseManifest(), fetchFn = fetch): Promise<string> {
  const data = await readManifest(source, fetchFn);
  if (!commit(expected) || commit(data.commit) !== expected.toLowerCase()) {
    resetReleaseFeedCache();
    throw new ReleaseChanged('The published build changed. Refresh the release details before updating.');
  }
  if (!hasServerKit(data)) throw new Error('The selected release is missing a server installer for this platform.');
  const assets = data.assets.map(asset => {
    if (typeof asset?.name !== 'string' || !/^[\w.-]+$/.test(asset.name) || ['.', '..'].includes(asset.name)
        || typeof asset.sha256 !== 'string' || !/^[a-f0-9]{64}$/i.test(asset.sha256)) {
      throw new Error('The selected release has an invalid asset name or checksum.');
    }
    const reference = asset.url ?? asset.name;
    const url = /^https?:\/\//i.test(source) ? new URL(reference, source).href
      : /^https?:\/\//i.test(reference) ? reference : resolve(dirname(source), reference);
    return { ...asset, url };
  });
  return JSON.stringify({ ...data, assets });
}

async function compareEdge(fetchFn: typeof fetch, repo: string, headers: Record<string, string> | null, current: string | null, latest: string | null) {
  if (!current || !latest) return 'unknown';
  if (current === latest) return 'identical';
  // Git SHAs have no sort order. A newer local build or a downstream fork
  // must not be offered an older or unrelated same-version release.
  try {
    const response = await fetchFn(`https://api.github.com/repos/${repo}/compare/${current}...${latest}?per_page=1`, {
      headers: headers ?? await githubHeaders(), signal: AbortSignal.timeout(10_000)
    });
    if (response.ok) {
      const { status } = await response.json() as { status?: unknown };
      if (status === 'ahead' || status === 'behind' || status === 'diverged' || status === 'identical') return status;
    }
  } catch { /* A comparison outage still leaves both version identities available. */ }
  return 'unknown';
}

/** The channel status for this server: what is running, what is published,
 * and whether an update is available. Transport failures throw so the HTTP
 * route can answer 502 instead of a silently stale banner. */
export async function getReleaseStatus(
  options: ReleaseFeedOptions = {}
): Promise<ReleaseChannelStatus> {
  const now = options.now ?? Date.now;
  const repo = options.repo ?? configuredRepo();
  const channel = options.channel ?? configuredChannel();
  const pin = options.manifest ?? process.env.PARASAUR_RELEASE_MANIFEST;
  const current = options.currentCommit === undefined ? runningBuild.revision : commit(options.currentCommit);
  const fetchFn = options.fetch ?? fetch;
  const key = JSON.stringify([repo, channel, pin, current]);
  if (cache?.key === key && now() - cache.at < CACHE_TTL_MS) return cache.status;

  const headers = pin ? null : await githubHeaders();
  const release = headers ? await latestChannelRelease(fetchFn, repo, channel, headers) : null;
  const status: ReleaseChannelStatus = {
    channel,
    current_version: COORDINATOR_VERSION,
    latest_version: null,
    current_commit: current,
    latest_commit: null,
    pinned: Boolean(pin),
    installable: false,
    update_available: false,
    release_url: null,
    published_at: null,
    notes: ''
  };
  let target: ReleaseDocument | undefined;
  if (pin) {
    target = await readManifest(pin, fetchFn);
  } else if (release) {
    status.release_url = release.html_url;
    status.published_at = release.published_at;
    status.notes = (release.body ?? '').slice(0, MAX_NOTES_CHARS);
    const asset = release.assets?.find(asset => asset.name === MANIFEST_ASSET_NAME && asset.state === 'uploaded');
    if (asset) {
      // The moving tag can be replaced between the metadata and manifest reads.
      target = asset.id
        ? await readManifest(`https://api.github.com/repos/${repo}/releases/assets/${asset.id}`, fetchFn, { ...headers, Accept: 'application/octet-stream' })
        : await readManifest(asset.browser_download_url, fetchFn);
    } else if (/^v?\d+(\.\d+){1,2}$/.test(release.tag_name)) {
      status.latest_version = release.tag_name.replace(/^v/, '');
    }
  }
  if (target) {
    status.installable = hasServerKit(target);
    status.latest_version = target.version;
    status.latest_commit = commit(target.commit);
    const order = compareVersions(target.version, COORDINATOR_VERSION);
    if (order === 0 && (channel === 'edge' || pin)) {
      status.revision_comparison = await compareEdge(fetchFn, repo, headers, current, status.latest_commit);
    }
    status.update_available = status.installable && (order > 0 || status.revision_comparison === 'ahead');
  }
  cache = { key, at: now(), status };
  return status;
}
