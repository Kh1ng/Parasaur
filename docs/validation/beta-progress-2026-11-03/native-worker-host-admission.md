# Native worker host admission

## Acknowledged capacity release

The owner-only central `/api/native-dispatch/acknowledgement` exchange now replays
the durable canonical acknowledgement for the exact run/node/context, original
holder and receipt hash. Pending import, a changed scope or receipt, browser
principals and revoked worker/profile membership are refused. The completed fact
survives restart and expiry and does not require fresh model permission after a
factory pause. Only fixed acknowledgement fields are returned.

Private Linux `native-handoff release-result` takes an existing service credential
on bounded private stdin, checks its configured authority against the original
manifest, and independently retrieves that acknowledgement. Caller-supplied
acknowledgements are not accepted. The worker synchronizes Acknowledged and
StopIntent records before stopping its exact holder. A fresh process can finish
the same stop after a lost reply; it never starts a job or stops a replacement
holder. The release journal retains the original scope and receipt identity.

Host admission now records the actual capacity lease path/device/inode before
descriptor transfer. Release pins that exact inode, verifies the job is empty
and the holder has no stored descriptors, and obtains the lease's free kernel
lock before synchronizing Released. The verifier descriptor remains held until
that barrier completes. The ordinary capacity scanner may subsequently collect
the free registry entry; a cold retry recognizes that completed state. Existing
capacity guards and registry permissions are preserved. Privacy comes from the
owned 0700 user runtime directory; ordinary worker lease files retain their
existing umask.

Only after acknowledgement and confirmed release are original context history
chunks removed, with private-file/hash checks and directory synchronization.
Working ledger and session evidence remain. Cleanup can retry independently;
result replay still returns the immutable receipt after context chunk removal.
The compact worker status now reports Released and free capacity only after a
new non-mutating holder/job/kernel-lease check, rather than leaving completed
jobs indistinguishable from unknown state.

The real WSL fixture refuses pending import, wrong acknowledgement and changed
capacity identity without stopping the holder. It then proves canonical import,
kernel release, context cleanup, repeated release/result/status calls and replay
refusal. A four-second fixture stop exceeds the fixed three-second reply budget;
the persisted StopIntent recovers after that same stop completes, with exactly
one stop invocation. The fixture uses controlled authority/backend/forge, not a
paid model. Central-to-worker production API wiring remains unfinished and no
native installed service is changed by this increment.

Worker writer guards now explicitly unlock on drop. This avoids a transient
false busy result when another thread forks with a copy of the descriptor just
before the writer finishes. A deterministic duplicated-descriptor check covers
the failure without changing capacity lease handoff lifetime.

Validation: 49 native library cases, 573 dispatch cases, 76 provider cases,
three CLI cases, 12 capability cases, formatting and all-target/all-feature
Clippy pass. Thirty-three broker/owner-HTTP cases and the server typecheck pass.
Seven actual WSL lifecycle cases pass, including the genuine stop reply timeout,
changed holder proof, acknowledgement refusal, kernel release and Released status.
Local metadata is retained at `artifacts/central-live-20261105/native-worker-release.jsonl`.
The independent host/descendant/crash check remains required for the final candidate.

## Terminal result recovery

The private Linux `native-handoff result --context-dir ... --binding-file ...
--work-dir ...` command recovers a result without starting, cancelling or releasing
anything. It requires the worker's own node identity and exact private journal.
First capture checks the original invocation and main PID, systemd's exit
code/signal and nonzero exit timestamp, the fixed job retention contract, and
an empty full job cgroup. A second observation must agree after reading the
ledger. Missing unit evidence, a live original process, live descendants or an
absent actual ledger result retain the job for reconciliation.

Original history is checked against the bound manifest's chunk hashes and
streamed past; recovery does not allocate the entire history. Added rows are
bounded, complete JSON lines. Exactly one row must match the run/profile/repo/
work identity. Changed history, duplicate results and truncated/oversized rows
are refused. The validated original row is embedded verbatim in the receipt,
preserving numeric text, defaults and future fields instead of normalizing them
before delivery.

`result.json` is a private, non-overwriting spool with the complete binding,
launched process identity, raw payload hash and canonical receipt hash. File and
directory synchronization precede the reply. Cold replay returns the same saved
bytes even if later ledger data changes or the exited unit is collected. Cached
reads repeat synchronization, so a prior uncertain directory barrier is not
assumed durable. Payload/identity/digest changes are refused. A saved receipt
describes the captured exit; it does not certify current free capacity.

The real-process worker fixture now recovers actual allowed and denied results,
passes them through the real Rust canonical receiver, and verifies recorded/
already-recorded acknowledgements with the same receipt digest. It also checks
that a running backend and a dead main with live descendants cannot produce a
receipt, cold replay survives lost unit metadata and changed ledger data, and
spool corruption is rejected. Capacity remains held after canonical import.
The HTTP authority/backend/forge are controlled fixtures; no real model or
installed service is used. Canonical acknowledgement delivery to the worker,
physical capacity release and central-to-worker API wiring remain unfinished.

Validation: 46 native library cases, 573 dispatch cases, 76 provider cases,
three native CLI cases, 12 capability cases, formatting and all-target/all-feature
Clippy pass. Six WSL process cases cover the actual start/worker/result commands
and canonical receiver with controlled authority/backend/forge; five recover an
actual result and the unsupported provider-claim case refuses before consumption.
The separate main-crash/live-descendant test also rejects result collection.
Local metadata: `artifacts/central-live-20261105/native-worker-result.jsonl`.
Rust log: `/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-dispatch-checks.log`.

## Fixed launch transport

The private Linux `native-handoff start` command now accepts the same bounded
stdin envelope as the fixed worker. It rechecks staged context/history and
worker identity, durably offers one start, then launches only the current CLI's
fixed `native-handoff execute` entry point. The transient UUID job uses Type=exec,
ExitType=cgroup, RemainAfterExit=yes, Restart=no and KillMode=control-group. It
does not replace an existing unit or wait for the entire model run. Its reply
confirms launch only; it explicitly does not confer execution authority.

Before credential delivery, the launcher records the observed invocation,
main PID and kernel start ticks under the worker journal lock. It then releases
that lock, allowing the wrapper to consume the grant. This avoids a startup race
where the child reaches its nonblocking journal lock while its parent still
owns it. Initial consumption requires this exact recorded launch, in addition
to the actual job/holder checks.

The journal now writes version 3. Version-1/2 state remains readable; existing
consumed version-2 identity is preserved when upgraded on an operation write.
Unstarted legacy fences are never reset and cannot consume without a recorded
launch. Old readers reject the new schema. Scope mismatch fails without changing
the journal.

Credentials are delivered over a bounded Unix socket in an owned private user
runtime directory. SO_PEERCRED must match the recorded main PID and user, and
the entire physical job identity is rechecked before any bytes are sent. A
different process under the same user receives no credential bytes. I considered
a private FIFO; PID-checked delivery avoids making its reader identity implicit.
The unit receives null stdin and no credential environment property. Logs are
created exclusively with mode 0600; existing log evidence is not truncated.
Systemd specifier characters in execution/output paths are refused before launch.
Failed replies, delivery, persistence or expiry retain StartIntent and capacity.

The real-process worker check now calls the actual start command; it no longer
injects StartIntent or launch records. Controlled central/backend fixtures cover
allowed execution, paused intake, denied publication, discarded launch replies
with cold start/execute replay refusal, and unsupported provider-claim mode.
Canonical result import/release and a central-to-worker API caller remain
unfinished; this command is local-only and is not a generic remote operation.
No installed worker, controller or API is updated by this increment.

Validation: 44 native library cases, 573 dispatch cases, 76 provider cases,
three native CLI cases, 12 capability cases, formatting and all-target/all-feature
Clippy pass. Five actual start/worker service cases pass, including discarded
launch output followed by cold replay refusal. The independent real holder/
writer-crash/main-crash-with-descendant test passes with its entry identity
explicitly controlled. The socket unit check rejects a wrong PID under the same
user without sending credential bytes. Local metadata is retained in
`artifacts/central-live-20261105/native-worker-launch.jsonl`; Rust log:
`/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-dispatch-checks.log`.

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
guards. Its initial stdin envelope goes to the real start command, which uses
the PID-verified delivery described above.
It demonstrates real start-consumption HTTP, actual scope installation and an
authorized fake-backend attempt; a paused factory starts no backend and records
no attempt; a denied publication preserves the executed backend result without
attempting a push. Operation fences, capacity retention after exit, empty job
cgroups, replay refusal and absence of credential plaintext in fixture artifacts
are checked. Provider-assignee claim mode is refused before consumption, and a
discarded launch response does not enable a second offer. Disposable units are
stopped afterward. StartIntent and launch identity come from the actual CLI.
Its HTTP authority and backend/forge are fixtures, not a production broker or
paid model run.

Provider-assignee claim mode is explicitly refused before start consumption.
Its background renewal lifecycle is not yet integrated with operation fences;
this avoids writing outside those fences or weakening the configured provider
claim policy. Central-to-worker API transport, provider-claim integration,
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
current authority or claiming an agent ran. The operation consumer introduced
journal version 2; the launch identity now requires version 3 as described above.
Legacy start fences are retained across upgrades. Older readers fail closed
rather than resetting the new state.

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

The fixed start CLI now uses this callback to launch the verified wrapper, which
consumes central's one-use grant and applies fresh later attempt/publication
authority. No worker HTTP start endpoint or generic remote command launch was
added. Terminal result spooling, reconciliation/release and central-to-worker
API transport remain unfinished.

## Retiring an unused reservation

An unused systemd holder does not physically expire with its short-lived start
offer. The former broker comment implying expiry was incorrect. The fixed
local-only command `gah native-handoff release-reservation --binding-file
<private binding> --work-dir <owned work> --reservation-id <original holder>`
now retires a verified, never-started reservation. It neither records a result
nor consumes execution authority.

The locked worker journal must be Ready, without any start, launch, execution,
consumption or operation record. A private, binding-scoped
`reservation-release.json` commits StopIntent before external effects. Its mere
presence prevents any subsequent start, including after a lost reply or cold
reload. The host must prove the job unit absent and its cgroup empty, verify
the original holder and exact registry inode, stop only that holder and obtain
the kernel lease lock before durably recording Released. An unknown/unrecorded
job or changed holder retains capacity. An uncertain write latches refusal.

Fresh worker status reports `reservation_released` and `capacity_held: false`
only after rechecking physical release. Repeated release and lost-stop-reply
recovery do not stop another holder. Context history stays intact, and no
canonical result or attempt is fabricated. Possible starts still require the
existing terminal result and canonical acknowledgement path instead.

Focused Linux verification passes 52 native, 573 dispatch, 76 provider, three
native CLI and 12 capability cases, formatting and all-target/all-feature
Clippy with warnings denied. Real WSL systemd scenarios pass ordinary unused
release, a four-second stop exceeding the command reply deadline, and the
existing main-process-crash/live-descendant retention scenario. They use default
4 GiB admission and disposable units, no model or real issue. An initial replay
fixture correctly refused an inherited ledger override; removing that test-only
override, as the fixed launch service does, reaches and verifies the cancellation
fence. No production guard was relaxed to make that assertion pass.

Central now durably records `cancelling` before requesting this operation. It
requires an exact matching worker proof of physical release and no dispatch
start before retiring the old central claim and recording `cancelled`. Failed
or lost replies retain the claim and retry only cancellation of the original
reservation. Neither fresh permission nor a stale Ready observation can restart
the cancelled run. Proof persistence precedes claim retirement; interrupted
claim cleanup replays without another worker release. A running/result-pending
claim and a newer work owner remain protected. No canonical result is invented
and original context history remains available.

All 44 focused broker cases, contracts build and server typechecking pass.
New cases cover exact proof, held capacity, lost replies, both uncertain writes
and preservation of running/newer claims. Production worker transport/startup
remain unfinished. No installed CLI, live factory hold, service or issue was
changed.

### Optimized and matched acceptance

The optimized `9501a798` CLI verifies all 625 Rust/Cargo inputs against its
immutable source archive. SHA-256:
`b82fd34aad9838660c6b7693617d215532553c68ca4bd0c0adabeb586b797c71`.
Artifact: `artifacts/parasaur-native-context-9501a798/gah-linux-x86_64`.
Release/context/current-policy/zero-spend smokes pass; it requires glibc 2.39
and is staged, not installed.

That exact binary passes all three real host scenarios and all seven terminal
execution/release scenarios. Evidence:
`artifacts/central-live-20261105/native-unused-release-optimized.jsonl`.
No disposable native capacity/job units remain after the checks.

Public `744fc811` / private `213f2be3` passes 1,219 server cases (38 existing
skips), 17 mock cases, production builds and release/input/credential/transport
guards. Log:
`/home/khing/.cache/parasaur-enterprise-current.w4q40507/build.log`.
Its Rust/Cargo inputs are identical to the optimized `9501a798` candidate.
The existing actual Rust/compiled owner-HTTP broker bridge passes again using
this pair and exact binary.

A paired `native_host_reservation.py ... release-unused-broker <node> <compiled
API directory>` / `native_reservation_broker.mjs` fixture runs the actual compiled
broker/claims and optimized worker CLI against a real systemd holder. Delivery
and denial authority are controlled. Its first release really frees the kernel
lock and deliberately loses the reply; central retains its claim. A fresh
broker/claims instance retries the same worker release, then retires the claim.
One stage, two release requests, zero starts/imports/models occur. Evidence:
`artifacts/central-live-20261105/native-unused-broker-optimized.json`.
The first combined fixture correctly refused an intermediate directory created
with the inherited umask; explicitly creating every fixture ancestor privately
reaches this test without weakening broker permissions.

This verifies the cancellation boundary across real broker and worker code,
not production network delivery, live factory progression or a memory leak fix.
