// Issue #882: /api/claims/* route + authorization layer (registered node,
// declared profile). Real fake HTTP calls against a real createServer()
// instance, matching this repo's existing route-test convention
// (server.test.ts's withTestServer).
import assert from 'node:assert/strict';
import { test } from 'node:test';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { createServer } from './server.js';
import { RegistryService } from './registryService.js';
import { ClaimsService } from './claimsService.js';
import { getCoordinatorIdentity, resetCachedCoordinatorIdentity, COORDINATOR_SCHEMA_DIGEST } from './coordinatorIdentity.js';
import type { ProfileSummary } from '@git-agent-harness/contracts';

function tempPath(prefix: string): string {
  return join(mkdtempSync(join(tmpdir(), prefix)), 'config.json');
}

async function withClaimsServer(
  setup: (registry: RegistryService) => void,
  testFn: (url: string, localNodeId: string) => Promise<void>,
  profiles?: () => Promise<ProfileSummary[]>
) {
  resetCachedCoordinatorIdentity();
  const registryService = new RegistryService(tempPath('gah-registry-'));
  const claimsService = new ClaimsService(tempPath('gah-claims-'));
  setup(registryService);

  const app = createServer({ registryService, claimsService, ...(profiles ? { runProfileList: profiles } : {}) });
  const server = http.createServer(app);
  await new Promise<void>((resolve) => server.listen(0, resolve));
  const { port } = server.address() as AddressInfo;

  try {
    await testFn(`http://127.0.0.1:${port}`, getCoordinatorIdentity().node_id);
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
    resetCachedCoordinatorIdentity();
  }
}

let nextTestPort = 10000;

const localProfiles = async () => [{ name: 'gah' } as ProfileSummary];
const claim = (url: string, action: string, nodeId: string, profile = 'gah') => fetch(`${url}/api/claims/${action}`, {
  method: 'POST', headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ node_id: nodeId, profile, work_id: 'shared-work' })
});

test('central and a registered worker arbitrate one claim without registering central as a worker', async () => {
  await withClaimsServer(
    registry => registerTestNode(registry, 'worker', ['gah']),
    async (url, central) => {
      const responses = await Promise.all([claim(url, 'acquire', central), claim(url, 'acquire', 'worker')]);
      assert.deepEqual(responses.map(response => response.status).sort(), [200, 409]);
      const winner = responses[0].status === 200 ? central : 'worker';
      const other = winner === central ? 'worker' : central;
      const conflict = await responses.find(response => response.status === 409)!.json() as { held_by: { node_id: string } };
      assert.equal(conflict.held_by.node_id, winner);
      assert.equal((await claim(url, 'renew', winner)).status, 200);
      assert.equal((await claim(url, 'release', other)).status, 200);
      assert.equal((await claim(url, 'acquire', other)).status, 409, 'another node cannot release the holder\'s claim');
      assert.equal((await claim(url, 'release', winner)).status, 200);
      assert.equal((await claim(url, 'acquire', other)).status, 200);
      assert.equal((await claim(url, 'renew', other)).status, 200);
      assert.equal((await claim(url, 'release', other)).status, 200);
    }, localProfiles
  );
});

test('central claim membership comes from its local profiles, not registry declarations', async () => {
  await withClaimsServer(
    registry => registerTestNode(registry, getCoordinatorIdentity().node_id, ['remote-only']),
    async (url, central) => {
      for (const action of ['acquire', 'renew', 'release']) assert.equal((await claim(url, action, central, 'remote-only')).status, 400);
      assert.equal((await claim(url, 'acquire', central)).status, 200);
      assert.equal((await claim(url, 'release', central)).status, 200);
    }, localProfiles
  );
});

test('unavailable central profile discovery refuses leases and recovers without blocking registered workers', async () => {
  let available = false;
  let malformed = false;
  await withClaimsServer(
    registry => registerTestNode(registry, 'worker', ['gah']),
    async (url, central) => {
      for (const action of ['acquire', 'renew', 'release']) assert.equal((await claim(url, action, central)).status, 503);
      malformed = true;
      assert.equal((await claim(url, 'acquire', central)).status, 503);
      assert.equal((await claim(url, 'acquire', 'worker')).status, 200);
      available = true;
      assert.equal((await claim(url, 'acquire', central)).status, 409, 'failed discovery did not grant an invisible lease');
      assert.equal((await claim(url, 'release', 'worker')).status, 200);
      assert.equal((await claim(url, 'acquire', central)).status, 200);
      assert.equal((await claim(url, 'release', central)).status, 200);
    }, async () => {
      if (available) return localProfiles();
      if (malformed) return { name: 'gah' } as unknown as ProfileSummary[];
      throw new Error('private discovery details');
    }
  );
});

function registerTestNode(registry: RegistryService, nodeId: string, profiles: string[]) {
  registry.registerNode({
    node_id: nodeId,
    display_name: nodeId,
    advertised_url: `http://127.0.0.1:${nextTestPort++}`,
    version: '0.1.0',
    schema_digest: COORDINATOR_SCHEMA_DIGEST,
    transport_mode: 'loopback',
    secret_ref: 'env:UNUSED',
    profiles
  });
}

test('a registered node that declared the profile can acquire a claim', async () => {
  await withClaimsServer(
    (registry) => registerTestNode(registry, 'node-a', ['gah']),
    async (url) => {
      const res = await fetch(`${url}/api/claims/acquire`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ node_id: 'node-a', profile: 'gah', work_id: 'ticket-1' })
      });
      assert.equal(res.status, 200);
      const body = (await res.json()) as { node_id: string };
      assert.equal(body.node_id, 'node-a');
    }
  );
});

test('an unregistered node_id is rejected, not silently granted a claim', async () => {
  await withClaimsServer(
    () => {},
    async (url) => {
      const res = await fetch(`${url}/api/claims/acquire`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ node_id: 'ghost-node', profile: 'gah', work_id: 'ticket-1' })
      });
      assert.equal(res.status, 400);
    }
  );
});

test('a registered node cannot claim a profile it never declared', async () => {
  await withClaimsServer(
    (registry) => registerTestNode(registry, 'node-a', ['sportsball']),
    async (url) => {
      const res = await fetch(`${url}/api/claims/acquire`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ node_id: 'node-a', profile: 'gah', work_id: 'ticket-1' })
      });
      assert.equal(res.status, 400);
      const body = (await res.json()) as { message: string };
      assert.match(body.message, /has not declared profile/);
    }
  );
});

test('a second node conflicting with an existing claim gets 409 with the holder identified', async () => {
  await withClaimsServer(
    (registry) => {
      registerTestNode(registry, 'node-a', ['gah']);
      registerTestNode(registry, 'node-b', ['gah']);
    },
    async (url) => {
      await fetch(`${url}/api/claims/acquire`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ node_id: 'node-a', profile: 'gah', work_id: 'ticket-1' })
      });
      const res = await fetch(`${url}/api/claims/acquire`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ node_id: 'node-b', profile: 'gah', work_id: 'ticket-1' })
      });
      assert.equal(res.status, 409);
      const body = (await res.json()) as { held_by: { node_id: string } };
      assert.equal(body.held_by.node_id, 'node-a');
    }
  );
});

test('renew and release round-trip through the real routes', async () => {
  await withClaimsServer(
    (registry) => registerTestNode(registry, 'node-a', ['gah']),
    async (url) => {
      const acquireRes = await fetch(`${url}/api/claims/acquire`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ node_id: 'node-a', profile: 'gah', work_id: 'ticket-1' })
      });
      assert.equal(acquireRes.status, 200);

      const renewRes = await fetch(`${url}/api/claims/renew`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ node_id: 'node-a', profile: 'gah', work_id: 'ticket-1' })
      });
      assert.equal(renewRes.status, 200);

      const releaseRes = await fetch(`${url}/api/claims/release`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ node_id: 'node-a', profile: 'gah', work_id: 'ticket-1' })
      });
      assert.equal(releaseRes.status, 200);

      // Released -- a different node can now acquire it.
      const reacquireRes = await fetch(`${url}/api/claims/acquire`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ node_id: 'node-a', profile: 'gah', work_id: 'ticket-1' })
      });
      assert.equal(reacquireRes.status, 200);
    }
  );
});

// Issue #882 / CodeQL js/missing-rate-limiting: these routes are
// authenticated but called frequently by design (renewals), so a generous
// per-IP limit exists as defense-in-depth against a buggy/compromised node
// rather than to throttle normal use -- this proves the limiter is
// actually wired in, not just configured and forgotten.
test('acquire is rate-limited past the configured per-IP window', async () => {
  await withClaimsServer(
    (registry) => registerTestNode(registry, 'node-a', ['gah']),
    async (url) => {
      let sawRateLimited = false;
      for (let i = 0; i < 65; i++) {
        const res = await fetch(`${url}/api/claims/acquire`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          // Distinct work_id per call -- isolating "rate limited" from
          // "conflicts with a claim this same loop already acquired".
          body: JSON.stringify({ node_id: 'node-a', profile: 'gah', work_id: `ticket-${i}` })
        });
        if (res.status === 429) {
          sawRateLimited = true;
          break;
        }
      }
      assert.ok(sawRateLimited, 'expected a 429 before 65 requests in one window');
    }
  );
});
