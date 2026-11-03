import assert from 'node:assert/strict';
import test from 'node:test';
import http from 'node:http';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { COORDINATOR_VERSION } from '@git-agent-harness/contracts';
import { COORDINATOR_SCHEMA_DIGEST } from './coordinatorIdentity.js';
import { RegistryService } from './registryService.js';
import { parseNodeCentralClaims } from './centralClaimObservation.js';

test('worker claim projection excludes secret origins, malformed fields and unknown success claims', () => {
  assert.deepEqual(parseNodeCentralClaims({ central_url: 'https://user:secret@central.test' }, 'project', 'healthy'), []);
  assert.deepEqual(parseNodeCentralClaims({ central_url: 'https://central.test?token=secret' }, 'project', 'healthy'), []);
  const rows = parseNodeCentralClaims({ central_url: 'https://central.test', central_claim_observation: {
    state: 'observed', outcome: 'future-success', observed_at: '2026-11-03T12:00:00Z', work_id: '#1', error: 'private-canary'
  } }, 'project', 'healthy');
  assert.equal(rows[0].observation?.state, 'observed');
  assert.equal(rows[0].observation?.state === 'observed' && rows[0].observation.outcome, 'unknown');
  assert.ok(!JSON.stringify(rows).includes('private-canary'));
  const invalid = parseNodeCentralClaims({ central_url: 'https://central.test', central_claim_observation: {
    state: 'observed', outcome: 'acquired', observed_at: 'invalid', work_id: '#1'
  } }, 'project', 'healthy');
  assert.equal(invalid[0].observation, undefined);
});

test('fleet carries per-profile claim history, preserves outage provenance, and replaces it on recovery', async () => {
  const root = mkdtempSync(join(tmpdir(), 'gah-fleet-claim-'));
  let firstState = 'refused';
  let firstOffline = false;
  let wrongProfile = false;
  const server = http.createServer((request, response) => {
    const profile = new URL(request.url!, 'http://fixture').searchParams.get('profile');
    if (firstOffline && profile === 'first') { response.writeHead(503); response.end(); return; }
    response.writeHead(200, { 'Content-Type': 'application/json' });
    response.end(JSON.stringify({
      version: COORDINATOR_VERSION, schema_digest: COORDINATOR_SCHEMA_DIGEST, generated_at: new Date().toISOString(),
      profile: { profile: wrongProfile && profile === 'first' ? 'another-project' : profile },
      node: { central_url: 'https://central.test', central_claim_observation: {
        state: 'observed', outcome: profile === 'first' ? firstState : 'acquired', observed_at: '2026-11-03T12:00:00Z', work_id: '#1516'
      } },
      nodes: [{ central_claim_observations: [{ profile: 'first', central_url: 'https://spoofed.test' }] }]
    }));
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const registry = new RegistryService(join(root, 'registry.json'));
  registry.registerNode({ node_id: 'remote-machine', display_name: 'Remote machine', advertised_url: `http://127.0.0.1:${address.port}`,
    transport_mode: 'loopback', secret_ref: 'env:UNUSED', profiles: ['first', 'second'], version: COORDINATOR_VERSION, schema_digest: COORDINATOR_SCHEMA_DIGEST });
  const read = async () => { await registry.getNodeObservations(); return registry.getCachedObservations()[0].central_claim_observations!; };
  try {
    let rows = await read();
    assert.deepEqual(rows.map(row => [row.profile, row.source_state]), [['first', 'healthy'], ['second', 'healthy']]);
    assert.ok(rows.every(row => row.central_url === 'https://central.test'), 'nested node observations are never relayed');
    assert.deepEqual(rows.map(row => row.observation?.state === 'observed' && row.observation.outcome), ['refused', 'acquired']);
    firstOffline = true;
    rows = await read();
    assert.deepEqual(rows.map(row => [row.profile, row.source_state]), [['first', 'unreachable'], ['second', 'healthy']]);
    assert.equal(rows[0].observation?.state === 'observed' && rows[0].observation.outcome, 'refused');
    firstOffline = false;
    wrongProfile = true;
    rows = await read();
    assert.equal(rows[0].source_state, 'incompatible', 'a response for another profile cannot refresh cached evidence');
    wrongProfile = false;
    firstState = 'acquired';
    rows = await read();
    assert.equal(rows[0].source_state, 'healthy');
    assert.equal(rows[0].observation?.state === 'observed' && rows[0].observation.outcome, 'acquired');
  } finally {
    server.closeAllConnections();
    await new Promise<void>(resolve => server.close(() => resolve()));
    rmSync(root, { recursive: true, force: true });
  }
});
