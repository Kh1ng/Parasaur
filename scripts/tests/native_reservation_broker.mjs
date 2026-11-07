/** Paired with native_host_reservation.py: actual broker/claims and worker CLI,
 * controlled delivery/authority, real systemd capacity, no backend or forge. */
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const [runtime, binary, fixture] = process.argv.slice(2);
const modules = pathToFileURL(`${runtime.replace(/\/$/, '')}/`);
const { NativeDispatchBroker } = await import(new URL('nativeDispatchBroker.js', modules));
const { ClaimsService } = await import(new URL('claimsService.js', modules));
const binding = JSON.parse(readFileSync(join(fixture, 'binding.json'), 'utf8'));
const root = join(fixture, 'runs');
const work = join(fixture, 'work');
const claimsPath = join(fixture, 'claims.json');
const reservation = JSON.parse(readFileSync(join(work, 'worker.json'), 'utf8')).holder_invocation;
const execute = promisify(execFile);
async function cli(action, ...args) {
  const result = await execute(binary, ['native-handoff', action, '--binding-file', join(fixture, 'binding.json'),
    '--work-dir', work, ...args], { timeout: 20_000, maxBuffer: 1024 * 1024 });
  return JSON.parse(result.stdout);
}
let claims = new ClaimsService(claimsPath);
let stages = 0, releases = 0;
const dependencies = () => ({
  root, centralUrl: 'https://native-host-fixture.invalid', coordinatorNodeId: 'fixture-central', claims,
  authorizeStart: async () => ({ allowed: false, reason: 'factory_paused' }),
  authorizeContinuation: async () => { throw Error('Cancelled work cannot request authority'); },
  recordCanonicalResult: async () => { throw Error('Unused reservation cannot invent a result'); },
  transport: {
    async stage() {
      stages++;
      const status = await cli('worker-status');
      assert.equal(status.state, 'ready'); assert.equal(status.capacity_held, true);
      return { ...binding, state: 'ready', reservation: { ...binding, id: reservation,
        expires_at: new Date(Date.now() + 20_000).toISOString() } };
    },
    async start() { throw Error('Cancelled work cannot start'); },
    async observe() { throw Error('Cancellation must reconcile its original reservation directly'); },
    async releaseReservation(_binding, held) {
      releases++;
      assert.equal(held.id, reservation);
      const result = await cli('release-reservation', '--reservation-id', held.id);
      assert.deepEqual(result.binding, binding);
      assert.equal(result.capacity_released, true); assert.equal(result.dispatch_started, false);
      if (releases === 1) throw Error('Fixture lost reply after actual physical release');
      return { ...result.binding, reservation_id: result.reservation_id,
        capacity_released: result.capacity_released, dispatch_started: result.dispatch_started };
    },
    async releaseResult() { throw Error('Unused reservation has no dispatch result'); },
  },
});
let broker = new NativeDispatchBroker(dependencies());
const pending = await broker.offerPrepared(binding.run_id, binding.context_sha256);
assert.equal(pending.phase, 'cancelling');
assert.ok(claims.getLease(binding.profile, binding.work_id)?.native_handoff);
assert.equal((await cli('worker-status')).state, 'reservation_released');
claims = new ClaimsService(claimsPath); broker = new NativeDispatchBroker(dependencies());
const cancelled = await broker.reconcile(binding.run_id);
assert.equal(cancelled.phase, 'cancelled');
assert.equal(cancelled.reservation_release.proof.capacity_released, true);
assert.equal(claims.getLease(binding.profile, binding.work_id), undefined);
assert.equal((await broker.offerPrepared(binding.run_id, binding.context_sha256)).phase, 'cancelled');
assert.equal(stages, 1); assert.equal(releases, 2);
console.log(JSON.stringify({ passed: true, actualBroker: true, actualWorkerCli: true,
  actualPhysicalRelease: true, claimRetainedUntilReplyRecovered: true,
  lostReleaseReplyRecoveredAcrossReload: true, controlledDeliveryAndAuthority: true,
  canonicalResultInvented: false, agentInvoked: false, stages, releases }));
