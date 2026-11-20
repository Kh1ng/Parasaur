// Called by native_host_reservation.py with a real, disposable systemd reservation.
import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const [dist, binary, root, reservation] = process.argv.slice(2);
process.env.PARASAUR_BINARY = binary;
process.env.PARASAUR_CLAIM_STATE_PATH = join(root, 'claims.json');
process.env.PARASAUR_WORKER_UPDATE_STATE_PATH = join(root, 'worker-update.json');
const { WorkerUpdateService } = await import(pathToFileURL(join(dist, 'workerUpdate.js')));
let launches = 0;
const service = new WorkerUpdateService({
  cwd: root,
  spawnFn: () => {
    launches++;
    return spawn('/usr/bin/true');
  },
});
const armed = await service.start();
console.log(JSON.stringify({ reservationHeld: true, update: armed.status.status, launches }));
assert.equal(armed.status.status, 'waiting');
assert.equal(armed.status.active_dispatches, 1);
assert.equal(launches, 0);
execFileSync(binary, ['native-handoff', 'release-reservation', '--binding-file', join(root, 'binding.json'),
  '--work-dir', join(root, 'work'), '--reservation-id', reservation]);
const deadline = Date.now() + 21_000;
while (service.status().status === 'waiting' && Date.now() < deadline) await delay(100);
while (service.status().status === 'running' && Date.now() < deadline) await delay(50);
assert.equal(service.status().status, 'success');
assert.equal(launches, 1);
console.log(JSON.stringify({ reservationReleased: true, update: 'success', launches }));
