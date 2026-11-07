# Native worker HTTP delivery

Public `fe2e57df` supplies the concrete `NativeWorkerHttpTransport`, worker
service and startup wiring. This replaces the previously missing transport;
it is staged source, not an installed factory rollout.

Central uses each registered worker's existing origin and secret reference.
Membership is checked before and after exchanges; redirects are not followed.
Only the owner service principal can call the two fixed worker POST routes.
The worker checks its node and local repository/profile identity independently.
No remote request selects an executable, local path, shell command or config.

Context transfer streams manifest-owned files to private temporary files,
checks exact size/digest, synchronizes data and directory entries and commits
each immutable file. Missing files resume after an interrupted upload.
Metadata is bounded to 8 MiB; history uses up to eight 128 MiB chunks. The
manifest admission checks free disk for two copies plus 64 MiB. This is not a
global disk-retention policy. A single API writer owns each private state root.
The full Rust loader independently verifies policy/history before admission.

The durable stage fence precedes the fixed systemd holder request. Short broker
observation deadlines do not repeatedly abort/restart a large transfer or
re-offer physical admission. Start is sent once, with its grant and central
credential delivered to the fixed CLI through private stdin. Subsequent HTTP
observations recover the real journal/result. Release endpoints run the exact
worker proof/acknowledgement commands; no HTTP success flag substitutes for
kernel release or canonical import.

Linux worker startup serves this execution API. Central startup constructs the
broker with the existing claims service, current authority checks, canonical
receiver and registered-node transport. After listening, one non-overlapping
ten-second poll streams durable runs without retaining completed runs in memory.
Shutdown stops the timer. The owner-only offer endpoint accepts only the UUID
and digest of an already-prepared context under the server-owned dispatch root;
the run observation endpoint omits credentials. No controller is started here.

## Verification

The final matched public `fe2e57df` / private `25a52fce` assembly passes 1,226
server cases, 38 existing skips, 17 mock cases, production server/shared-web
builds and release/input/credential/transport guards. Initial assembly refused
an overlapping startup insertion; the reviewed private hook placement preserves
both cloud hooks and native reconciliation. Log:
`/home/khing/.cache/parasaur-enterprise-current.qr41dm_3/build.log`.

Focused Windows checks pass 53 broker/context/HTTP cases and server typechecking.
These include interrupted upload recovery, immutable files, wrong digest/size,
private partial-file cleanup, service authentication, membership revocation,
owner-only offers/observations and periodic recovery without another start.
Two final broker tests were added after the matched assembly; application
source is unchanged by those test additions.

All Rust/Cargo inputs remain identical to optimized `9501a798`, SHA-256
`b82fd34aad9838660c6b7693617d215532553c68ca4bd0c0adabeb586b797c71`.
The same binary and compiled private API pass these opt-in WSL checks:

- `native_host_reservation.py <gah> http-broker <node> <compiled API>` runs
  the actual broker, production authentication, real HTTP/context transfer,
  worker service, default 4 GiB systemd admission and kernel release. Losing
  the first physical-release reply preserves central ownership. A new broker
  recovers the proof and retires only that claim; no backend starts.
- `native_worker_execution.py <gah> <node> <compiled API>` runs allowed and
  denied-attempt cases through the actual HTTP worker. Start consumption,
  process scope, durable result recovery, actual canonical receiver replay,
  wrong/pending acknowledgement refusal, changed-holder refusal, physical
  release, context cleanup and cold start/execute replay refusal pass. A
  controlled central authority server and disposable backend/forge are used;
  this is not a real model run or an actual central-controller selection.

The second check restarts its HTTP service between operations to exercise
on-disk recovery. Credentials stay on stdin/in process memory and are checked
absent from fixture artifacts. The unused-reservation test instead reloads the
actual broker/claims while retaining the worker service. Local evidence:
`artifacts/central-live-20261105/native-worker-http-broker.json` and
`native-worker-http-execution.jsonl` in the same directory.

## Remaining rollout boundary

The factory controller still needs to select a compatible worker, prepare the
bound context under this dispatch root and submit its offer. Provider-assignee
mode remains explicitly unsupported. A start that never established consumed
execution/result evidence still retains its reservation for reconciliation;
timeouts do not fabricate a result or zero usage. These boundaries must be
resolved before production native dispatch is enabled for live issues.

No installed CLI, API, controller, desktop app or live claim was changed by this
increment. It does not establish sustained-load memory acceptance, a leak fix,
GitLab sign-in persistence or completion of the desktop/shared-web goal.
