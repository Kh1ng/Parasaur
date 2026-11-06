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

- All 19 native-context/host tests, 17 host-capacity tests and two actual CLI
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
