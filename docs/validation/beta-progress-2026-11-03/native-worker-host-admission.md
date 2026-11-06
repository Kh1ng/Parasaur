# Native worker host admission

## Fixed worker executable

The private Linux `native-handoff execute --context-dir ... --expected-context
... --work-dir ... --config-path ...` entry point now consumes an already-offered
start from its actual job service, installs process authority and calls ordinary
dispatch. It accepts no arbitrary executable or alternate central origin.
The command is security-sensitive, local-only and non-idempotent; no generic
remote operation can invoke it.

Its strict credential envelope arrives only on noninteractive stdin, bounded
to 16 KiB and two seconds. The wrapper disables core dumps and process memory
access before reading it, and closes/replaces stdin with `/dev/null` before any
subprocess. Credentials are not serialized as config, arguments or artifacts.
Malformed/oversized input returns static errors before configuration loading.

Execution revalidates the original manifest, policy, request and each source
history chunk. The existing private job ledger must match those chunks exactly,
including EOF; additional entries and changed repository policy refuse execution.
This reads the staged history in bounded chunks without creating another copy
or overwriting earlier job evidence. The ordinary ledger path retains actual
results. The wrapper cannot certify its own death or release capacity; a parent
must independently observe terminal service/cgroup state before building a
terminal receipt.

A refused operation now has a distinct error type. Fix/PM workflows propagate
that hold before backend launch-failure bookkeeping. The real-process fixture
exposed the prior false-attempt record, which is now prevented; a held fix also
preserves earlier work before cleaning its worktree.

`scripts/tests/native_worker_execution.py` exercises the compiled executable
under actual WSL systemd job/capacity units with the default physical capacity
guards. A sealed anonymous memfd supplies credential stdin through systemd's
[file input support](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html#StandardInput=).
It demonstrates real start-consumption HTTP, actual scope installation and an
authorized fake-backend attempt; a paused factory starts no backend and records
no attempt; a denied publication preserves the executed backend result without
attempting a push. Operation fences, capacity retention after exit, empty job
cgroups, replay refusal and absence of credential plaintext in fixture artifacts
are checked. A fourth case verifies refusal of provider-assignee claim mode
before consumption. Disposable units are stopped afterward. The fixture's initial
StartIntent is controlled, not proof of an implemented launch/offer transport.
Its HTTP authority and backend/forge are fixtures, not a production broker or
paid model run.

Provider-assignee claim mode is explicitly refused before start consumption.
Its background renewal lifecycle is not yet integrated with operation fences;
this avoids writing outside those fences or weakening the configured provider
claim policy. Production launch/credential transport, provider-claim integration,
terminal result/release reconciliation and rollout remain unfinished. No
installed CLI, controller or API was changed by this increment.

Validation: 41 native library cases, 573 dispatch cases, 76 provider cases,
three native CLI cases, 12 capability cases, formatting and all-target/all-feature
Clippy with warnings denied pass. All four real-process fixture cases pass with
the compiled debug CLI. Metadata-only output is retained locally at
`artifacts/central-live-20261105/native-worker-execution.jsonl`; Rust check log:
`/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-dispatch-checks.log`.

## Worker-side permission consumption

The staged worker now has real control clients for initial grant consumption
and subsequent operation prerequisites. They use the central origin from the
already verified worker context and require its full binding to match the
journal. Start-consumption intent is synchronized before HTTP. A matching,
unexpired consumed-start response can enter the job once, bound to the actual
systemd job invocation, main PID, kernel process start ticks and unified cgroup.
Re-exec/replacement cannot inherit that entry. The fixed executable and dispatch
hooks described above now consume this library plumbing; production launch and
recovery transport remain unfinished.

Each operation records Pending before HTTP. The entire reply scope, purpose,
sequence, timestamps and ten-second maximum validity must match. The physical
holder and current job process are rechecked after the reply, and Authorized is
synchronized before returning a fresh permission. Lost replies, bad scopes,
expired replies and uncertain writes remain fenced. Another operation requires
explicit completion of the exact authorized permission. There is no Drop-based
completion: callers must first prove backend termination or finished
publication. This module does not itself provide that backend/result proof.

Only one operation record is retained per run. Diagnostic views distinguish
Pending, Authorized and Completed without exposing stored permission data as
current authority or claiming an agent ran. The worker journal writes version
2; legacy version-1 start fences are retained and upgraded on the next write.
Older readers fail closed rather than resetting the new state.

Control requests use fixed `/usr/bin/curl`, stdin-only credentials/JSON, a
five-second deadline, direct configured-authority connections and normal TLS
verification. No insecure fallback is added. Body validation caps replies at
64 KiB; a kernel file-size limit bounds both captured streams even for replies
with no declared size. Known central refusal codes are retained, while arbitrary
remote bodies/reasons are omitted from errors. Curl config discovery and TLS
key logging are disabled for this path.

I considered relying on curl's download-size option; its behavior for unknown
lengths differs before 8.4.0, so the fixed Linux capture uses a kernel limit
instead. See [curl's size-limit documentation](https://curl.se/docs/manpage.html#--max-filesize).
Oversized writes use an ignored SIGXFSZ disposition and disabled core files;
the [kernel resource-limit rules](https://man7.org/linux/man-pages/man2/getrlimit.2.html)
and [exec signal-disposition rules](https://man7.org/linux/man-pages/man2/execve.2.html)
support that behavior. Actual unknown-length reply tests pass without returning
remote data in the error.

Validation: 33 native context/host/journal/operation cases, eight HTTP-helper
cases, two actual CLI cases, formatting and all-target/all-feature Clippy with
warnings denied pass. The HTTP cases include real loopback POST/body/header
capture, oversized responses without a declared length, stalled peers and
unsafe inputs. State cases substitute the physical checker; they prove lost
reply/reopen fences, scope/expiry refusal, explicit completion, persistence
failure, safe denial messages and version-1 migration, not backend execution.
Log: `/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-operations-checks.log`.

The expanded real Windows WSL process check also passes using default capacity
settings. With controlled entry records and a real disposable job service,
the compiled CLI reports Running only for the exact invocation/PID/start ticks.
Changed identity or dead main process becomes Unknown even when descendants
remain; the independent capacity lock stays held until the disposable job is
empty and its holder is explicitly stopped. This is not a real central start
exchange or model run. No installed CLI/API/service is changed, and the previous
optimized candidate does not contain this increment.

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

Optimized candidate `09504779` verifies all 611 Rust/Cargo inputs against its
immutable archive. Formatting, release build, context/current-spend smokes and
the expanded real host/journal/crash check pass with that exact release binary.
SHA-256 is
`df58ffcfeece167407dca286a986a71870446e4225dfd256f3655474b7179019`;
the staged artifact is `artifacts/parasaur-native-context-09504779/` and requires
glibc 2.39. Build log:
`/home/khing/.cache/parasaur-native-context-release.nziiyi2e/build.log`.

That same optimized binary passes the real Rust/compiled HTTP broker bridge,
including one-use grants across restart, failed-result/unknown-cost import and
canonical history cleanup. Server/contracts source is verified unchanged since
`aa8f7ad8`, so its already-built private `bf75f1a` API runtime is reused; no new
full enterprise assembly is claimed. Provider/controller/worker transport are
controlled fixtures in this bridge. It runs no model or real issue. The host
crash check independently uses actual WSL systemd, kernel locks and default
capacity settings. Neither check installs the staged CLI.

## Process-scoped dispatch guards

The native execution module now installs authority once per dedicated process,
only after checking an entered worker journal against the actual job service,
PID/start ticks and separate holder. Serialized dispatch arguments and ambient
environment variables cannot activate it. The complete dispatch configuration
and request must match the verified context. History overrides are checked again
after loading the profile environment file.

Existing backend entry points and fix/review/PM workflow calls now request an
operation permission before invoking a backend. Attempt counters and route
records for the main fix/review loops are written inside that permission gate;
denial cannot consume those counters. A different account/model cannot replace
the bound identity in the ledger. Ordinary dispatch keeps its existing claim
path; an installed native process uses the already-held native claim.

Fix push/MR publication, review comments/findings/labels, source issue comments
and closure, and PM plan publication use the publication gate. Nested provider
helpers share only the current thread's logical publication and still verify
the profile. A caught nested failure prevents the outer operation fence from
being completed. A soft failed findings publication also retains uncertainty.

Explicit completion requires the job cgroup to contain only its wrapper PID.
Unconfirmed cleanup or journal completion blocks later operations while keeping
the actual backend exit/usage result available for ledger recording. Final
dispatch cannot report success while completion is unconfirmed.

These are source-level guards. Their focused fixtures demonstrate denial before
callback/bookkeeping, preservation of real results when completion fails,
identity mismatch rejection, nested-failure fencing and refusal to install from
an unentered journal. They do not demonstrate a successful installed native
process or a real model/forge transaction. The fixed executable/process checks
above add that process-scope evidence with controlled endpoints/backends;
production launch transport and the full result/release lifecycle remain
required before rollout.

Windows WSL validation passes: 38 native library cases, 573 dispatch cases,
76 provider cases, two native CLI integration cases, formatting and
`cargo clippy --all-targets --all-features -- -D warnings`. Local full log:
`/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-dispatch-checks.log`.

The launch callback is still a library primitive, not a production executor.
It must launch only the fixed verified wrapper, which consumes central's one-use
grant and applies fresh later attempt/publication authority. No worker start
endpoint or generic command launch was added. Actual wrapper launch, result
spooling, reconciliation/release and central transport remain unfinished.
