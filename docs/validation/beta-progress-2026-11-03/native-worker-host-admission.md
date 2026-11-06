# Native worker host admission

The private Linux `native-handoff host-reserve` command loads the verified
central context using the worker's own identity, credentials and capacity
settings. It reserves the real host's projected memory/CPU through the same
registry, pressure checks and safety floor used by local controller workers.
Implementation/repair, review and PM retain their existing reservation sizes;
unsupported native modes cannot reserve. No backend or forge operation runs.

The command must be the main process of a transient, run-UUID-named systemd
oneshot holder. It checks its actual unified cgroup, invocation, PID and exact
retention properties before transferring the locked lease FD. The manager must
confirm one stored FD under the same identity before a response is emitted.
That response explicitly says `execution_authorized: false`; a caller must
also observe active/exited, no main process and one stored FD after completion.
It is not an execution grant or a production worker transport.

The separate holder uses `RemainAfterExit=yes`, `FileDescriptorStoreMax=1`,
`FileDescriptorStorePreserve=yes`, `NotifyAccess=all`, `Restart=no`,
`StopWhenUnneeded=no` and control-group kill mode. Its short setup process exits;
systemd retains the lock without a resident reservation process. The lease
continues using the original plain memory/CPU file format, so existing/older
admission scanners count it correctly. Transfer uncertainty leaves the path
intact. A scanner reclaims it only after the last descriptor has released its
kernel lock, including a failed transfer that stored no descriptor.

A disposable WSL systemd 255 experiment rejected putting the lock in the job's
own FD store: stopping/collecting that unit closed the store. An inactive
transient holder without `RemainAfterExit` was also collected. The separate
active/exited holder survives the independent job's main-process crash and
its complete stop. These are observed installed-version results, not an
assumption about newer systemd versions.

The eventual worker must serialize durable start/release state per run and
prove that its distinct job cgroup is empty and no start is pending before
stopping this holder. This command deliberately exposes no generic release
endpoint: cgroup absence alone cannot rule out a queued start. Disconnect,
expired offers and lost replies are not release evidence. A holder left by
uncertain recovery must remain accounted until reconciled.

## Verification

- All 25 native-context/host/journal tests, 17 host-capacity tests and two actual CLI
  integration cases pass. The CLI case refuses admission outside its holder
  even with a valid prepared context. All-target/all-feature Clippy with
  warnings denied and Rust formatting pass.
- The retained-lock regression fills projected capacity while a transferred
  descriptor remains alive, refuses the next admission, and recovers only
  after that last owner closes it. The original encoding remains unchanged.
- `python3 scripts/tests/native_host_reservation.py /absolute/path/to/gah`
  passes on Windows Ubuntu-24.04 WSL/systemd 255 using actual default host
  admission (4 GiB implementation reservation and the configured adaptive
  floor). It prepares a real fixture context and observes the compiled command's
  holder active/exited with MainPID zero and one stored descriptor. A separate
  disposable job retains a descendant after its main process is killed; the
  capacity lock survives both that crash and the job's subsequent complete
  stop. Only stopping the independent holder unlocks it. No agent is invoked.
  Fixture units are stopped/collected; the installed worker remains running.
- Local Rust check log:
  `/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-host-checks.log`.

Nothing is installed or enabled. Production worker transport, durable
start/recovery/release, fresh attempt/publication authority and the central
controller caller remain required before real native jobs can execute.

## Durable worker start fences and recovery inspection

Host admission now writes private `worker.json` beside the verified job history.
It synchronizes reservation intent before transferring the descriptor and Ready
before emitting the response. A stable, non-renamed `worker.lock` excludes other
stage/start/recovery writers across processes. Existing journals cannot be
overwritten. Reopening requires the entire expected binding, including account,
model, repository and context identity; private-file checks and input limits
remain in force.

`WorkerRun::offer_start` requires a current exact holder and matching unexpired
grant before durably recording StartIntent. It stores only the token's SHA-256
and expiry. Only after file and directory synchronization can the supplied
launch callback run. Failed/lost replies, a replacement grant, reopen or a write
error cannot offer a second start. Persistence failures latch the in-memory
writer; recovery reads the actual committed journal. No timeout removes this
fence, and this module supplies no release/reset operation.

The local-only `native-handoff worker-status --binding-file ... --work-dir ...`
checks the worker's configured node identity, acquires that same lock and
combines the journal with current systemd state. Ready requires the exact
active/exited holder invocation, retained descriptor and retention contract.
Unavailable, stopped or replaced holders return unknown, with capacity held
also unknown rather than falsely reporting released capacity. StartIntent is
unknown until the future executor reconciles the real job and result.

Six journal cases cover lost reply/cold reopen, replacement and invalid/expired
grants, competing writer exclusion, reservation uncertainty, failed persistence,
binding mismatch, malformed state, symlink locks and missing holders. Focused
state tests substitute the holder check; the public start method rejects a
missing real holder. The expanded real Windows WSL process check additionally
verifies the compiled host's Ready journal, actual CLI recovery inspection,
cross-process lock exclusion and writer-crash recovery without state changes.
After the disposable holder stops, that CLI returns unknown. All focused checks
and all-target/all-feature Clippy with warnings denied pass.

Both Linux host commands are explicitly listed as security-sensitive,
local-only capability operations. Host reservation is marked non-idempotent;
no generic remote operation exposes these paths. All 12 capability integration
checks and all-target/all-feature Clippy with warnings denied pass.

The launch callback is still a library primitive, not a production executor.
It must launch only the fixed verified wrapper, which consumes central's one-use
grant and applies fresh later attempt/publication authority. No worker start
endpoint or generic command launch was added. Actual wrapper launch, result
spooling, reconciliation/release and central transport remain unfinished.
