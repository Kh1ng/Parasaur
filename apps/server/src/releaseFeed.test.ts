import assert from 'node:assert/strict';
import { test } from 'node:test';
import { getReleaseStatus, resetReleaseFeedCache, selectReleaseManifest, ReleaseChanged } from './releaseFeed.js';
import { COORDINATOR_VERSION } from '@git-agent-harness/contracts';

interface ReleaseFixture {
  tag_name: string;
  prerelease?: boolean;
  draft?: boolean;
  published_at?: string;
  body?: string;
  /** When set, the release carries an edge-manifest.json asset naming this version. */
  manifestVersion?: string;
  commit?: string;
}

function jsonResponse(value: unknown, ok = true): Response {
  return new Response(JSON.stringify(value), {
    status: ok ? 200 : 500,
    headers: { 'content-type': 'application/json' }
  });
}

const platform = process.platform === 'darwin' ? 'macos' : process.arch === 'arm64' ? 'linux-aarch64' : 'linux-x86_64';
const serverAssets = ['cli', 'mcp-server', 'server-bundle'].map(kind => ({
  kind, name: `${kind}-${platform}`, sha256: 'd'.repeat(64)
}));

/** A fetch fake serving one releases list plus the manifests the listed
 * releases link to. */
function fetchWithReleases(releases: ReleaseFixture[], comparison?: string): typeof fetch {
  const asGithub = (release: ReleaseFixture) => ({
    tag_name: release.tag_name,
    prerelease: release.prerelease === true,
    draft: release.draft === true,
    html_url: `https://github.com/Kh1ng/git-agent-harness/releases/tag/${release.tag_name}`,
    published_at: release.published_at ?? null,
    body: release.body ?? '',
    assets: release.manifestVersion
      ? [{
          name: 'edge-manifest.json',
          browser_download_url: 'https://github.com/Kh1ng/git-agent-harness/releases/download/edge/edge-manifest.json',
          state: 'uploaded'
        }]
      : []
  });
  const listed = releases.map(asGithub);
  return (async (input: string | URL) => {
    const url = String(input);
    if (url.includes('/compare/')) return jsonResponse({status:comparison}, Boolean(comparison));
    if (url.endsWith('/releases/tags/edge') || url.endsWith('/releases/latest')) {
      const channelRelease = listed.find(r => !r.draft && (url.endsWith('/edge') ? r.tag_name === 'edge' : !r.prerelease));
      return channelRelease ? jsonResponse(channelRelease) : new Response(null, {status:404});
    }
    if (url.endsWith('/edge-manifest.json')) {
      // The chosen release's manifest: serve the fixture's version.
      const version = releases.find((release) => release.manifestVersion)?.manifestVersion ?? '0.0.0';
      return jsonResponse({ schema: 1, version, commit: releases.find(r => r.manifestVersion)?.commit, channel: 'edge', assets: serverAssets });
    }
    return jsonResponse({ error: 'unexpected fetch' }, false);
  }) as unknown as typeof fetch;
}

test('getReleaseStatus reports an available edge release newer than this server', async () => {
  resetReleaseFeedCache();
  const status = await getReleaseStatus({
    fetch: fetchWithReleases([
      {
        tag_name: 'edge',
        prerelease: true,
        published_at: '2026-10-05T00:00:00.000Z',
        body: 'Release notes body',
        manifestVersion: '99.0.0'
      }
    ]),
    now: () => 1_000
  });
  assert.equal(status.channel, 'edge');
  assert.equal(status.current_version, COORDINATOR_VERSION);
  assert.equal(status.latest_version, '99.0.0');
  assert.equal(status.update_available, true);
  assert.equal(status.release_url, 'https://github.com/Kh1ng/git-agent-harness/releases/tag/edge');
  assert.equal(status.notes, 'Release notes body');
});

test('getReleaseStatus ignores stable releases and drafts on the edge channel', async () => {
  resetReleaseFeedCache();
  const status = await getReleaseStatus({
    fetch: fetchWithReleases([
      { tag_name: 'v0.1.2', prerelease: false, published_at: '2026-10-04T00:00:00.000Z' },
      { tag_name: 'v0.2.0', prerelease: false, draft: true, published_at: '2026-10-05T00:00:00.000Z' }
    ]),
    now: () => 2_000
  });
  assert.equal(status.latest_version, null);
  assert.equal(status.update_available, false);
});

test('release metadata selects its exact manifest asset even if the tag moves', async () => {
  resetReleaseFeedCache();
  const api = 'https://api.github.com/repos/owner/project/releases';
  const selected = 'a'.repeat(40);
  const urls: string[] = [];
  const fetchFn: typeof fetch = async (input, init) => {
    const url = String(input);
    urls.push(url);
    if (url === api + '/tags/edge') {
      return jsonResponse({
        tag_name: 'edge', prerelease: true, draft: false, body: '',
        assets: [{ id: 123, name: 'edge-manifest.json', state: 'uploaded', browser_download_url: 'https://example.invalid/moving' }]
      });
    }
    assert.equal(url, api + '/assets/123');
    assert.equal(new Headers(init?.headers).get('Accept'), 'application/octet-stream');
    return jsonResponse({ schema: 1, channel: 'edge', version: COORDINATOR_VERSION, commit: selected, assets: serverAssets });
  };
  const status = await getReleaseStatus({ repo: 'owner/project', currentCommit: selected, fetch: fetchFn });
  assert.equal(status.latest_commit, selected);
  assert.deepEqual(urls, [api + '/tags/edge', api + '/assets/123']);
});

test('the edge feed selects the edge tag even when another prerelease is more recent', async () => {
  resetReleaseFeedCache();
  const status = await getReleaseStatus({
    fetch: fetchWithReleases([
      { tag_name: 'desktop-preview', prerelease: true, published_at: '2026-10-03T00:00:00.000Z', body: 'desktop' },
      { tag_name: 'edge', prerelease: true, published_at: '2026-10-01T00:00:00.000Z', body: 'server edge' }
    ]),
    now: () => 3_000
  });
  assert.equal(status.notes, 'server edge');
  assert.equal(status.latest_version, null);
  assert.equal(status.installable, false);
});

test('getReleaseStatus falls back to the tag when the release has no manifest asset', async () => {
  resetReleaseFeedCache();
  const status = await getReleaseStatus({
    fetch: fetchWithReleases([
      { tag_name: 'v0.0.1', published_at: '2026-10-05T00:00:00.000Z' }
    ]),
    now: () => 4_000, channel:'stable'
  });
  assert.equal(status.latest_version, '0.0.1');
  assert.equal(status.update_available, false);
});

test('getReleaseStatus serves a cached answer within the TTL and refetches after it', async () => {
  resetReleaseFeedCache();
  let calls = 0;
  let body = 'first';
  const fetchFn = (async () => {
    calls += 1;
    return jsonResponse(
      {
        tag_name: 'edge',
        prerelease: true,
        draft: false,
        html_url: 'https://github.com/Kh1ng/git-agent-harness/releases/tag/edge',
        published_at: '2026-10-05T00:00:00.000Z',
        body,
        assets: []
      }
    );
  }) as unknown as typeof fetch;

  let clock = 0;
  const first = await getReleaseStatus({ fetch: fetchFn, now: () => clock });
  assert.equal(first.notes, 'first');
  clock += 60_000; // one minute: inside the five-minute TTL.
  const cached = await getReleaseStatus({ fetch: fetchFn, now: () => clock });
  assert.equal(cached.notes, 'first');
  assert.equal(calls, 1);

  body = 'second';
  clock += 5 * 60_000; // past the TTL.
  const refreshed = await getReleaseStatus({ fetch: fetchFn, now: () => clock });
  assert.equal(refreshed.notes, 'second');
  assert.equal(calls, 2);
});

test('getReleaseStatus throws on transport failure so the route can answer 502', async () => {
  resetReleaseFeedCache();
  const fetchFn = (async () => {
    throw new Error('network down');
  }) as unknown as typeof fetch;
  await assert.rejects(
    getReleaseStatus({ fetch: fetchFn, now: () => 5_000 }),
    /network down/
  );
});

test('equal-version edge builds are offered only when the published commit descends from the installed one', async () => {
  const current = 'a'.repeat(40);
  const published = 'b'.repeat(40);
  for (const relation of ['ahead', 'behind', 'diverged', 'unavailable']) {
    resetReleaseFeedCache();
    const status = await getReleaseStatus({
      fetch: fetchWithReleases([{tag_name:'edge', prerelease:true, manifestVersion:COORDINATOR_VERSION, commit:published}],
        relation === 'unavailable' ? undefined : relation),
      currentCommit: current
    });
    assert.equal(status.current_commit, current);
    assert.equal(status.latest_commit, published);
    assert.equal(status.revision_comparison, relation === 'unavailable' ? 'unknown' : relation);
    assert.equal(status.update_available, relation === 'ahead');
  }
  for (const [installed, relation] of [[published,'identical'],[null,'unknown']] as const) {
    let compares = 0;
    const fetchFn = fetchWithReleases([{tag_name:'edge', prerelease:true, manifestVersion:COORDINATOR_VERSION, commit:published}]);
    const status = await getReleaseStatus({currentCommit:installed, fetch: async (input, init) => {
      if (String(input).includes('/compare/')) compares++;
      return fetchFn(input, init);
    }});
    assert.equal(status.revision_comparison, relation);
    assert.equal(status.update_available, false);
    assert.equal(compares, 0);
  }
});

test('a pinned feed supplies the displayed build and snapshots the selected asset locations', async () => {
  const selected = 'c'.repeat(40);
  const source = 'https://fleet.example/edge/manifest.json';
  const manifest = { schema: 1, channel: 'edge', version: COORDINATOR_VERSION, commit: selected, assets: serverAssets };
  const fetchFn: typeof fetch = async input => {
    assert.equal(String(input), source);
    return jsonResponse(manifest);
  };
  const status = await getReleaseStatus({ currentCommit: selected, manifest: source, fetch: fetchFn });
  assert.equal(status.pinned, true);
  assert.equal(status.revision_comparison, 'identical');
  const snapshot = JSON.parse(await selectReleaseManifest(selected, source, fetchFn));
  assert.deepEqual(snapshot.assets, serverAssets.map(asset => ({ ...asset, url: new URL(asset.name, source).href })));
  await assert.rejects(selectReleaseManifest('a'.repeat(40), source, fetchFn), ReleaseChanged);
  const incomplete: typeof fetch = async () => jsonResponse({ ...manifest, assets: serverAssets.slice(1) });
  await assert.rejects(selectReleaseManifest(selected, source, incomplete), /missing a server installer/);
});

test('getReleaseStatus truncates very long release notes', async () => {
  resetReleaseFeedCache();
  const notes = 'x'.repeat(20_000);
  const status = await getReleaseStatus({
    fetch: fetchWithReleases([
      { tag_name: 'edge', prerelease: true, published_at: '2026-10-05T00:00:00.000Z', body: notes }
    ]),
    now: () => 6_000
  });
  assert.equal(status.notes.length, 8_000);
});
