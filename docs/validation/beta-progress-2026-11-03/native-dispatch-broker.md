# Durable native dispatch broker

## Sequenced continuation prerequisites

The staged broker now renews central prerequisites for one `attempt` or
`publish` operation after initial start consumption. A mandatory continuation
callback checks the live controller identity before and after the real current
policy/spend and exact-route CLI check. `check-current-policy --check-route`
shares initial work revalidation's policy/runtime restrictions and live routing
approval checks, without requiring that running work still head the queue.
It returns `execution_authorized: false`; target/generation validation, attempt
limits and the full publication workflow remain worker responsibilities.

The broker requires its exact consumed start, holder identity, prepared context
and held native claim before and after that asynchronous check. It accepts only
the next integer sequence and persists a single last-operation record before
returning a permission with at most ten seconds of validity. Concurrent calls,
cold replay, forged identities, changed claims, terminal runs and uncertain
writes cannot return a reusable permission. Lost replies burn the sequence.
Denials retain the claim and do not spend a sequence. The original reservation's
start-offer expiry does not end an already running job's capacity lifetime;
the worker must affirmatively re-observe its actual independent holder.

Conditional `POST /api/native-dispatch/operation` accepts six bounded fields:
run/node/context/holder identity, sequence and purpose. Service-owner auth and
current registered profile membership are mandatory, including a second
membership check after the asynchronous decision. A revoked worker may burn a
sequence but never receive permission. There are no arbitrary commands, paths,
credential values or cached-permission responses. This endpoint remains absent
from default production startup until the native broker/worker is wired.

All 31 broker/HTTP cases pass on Windows, all four concrete-authority cases pass
on WSL, and server typechecking passes. Rust's 25 native context/host/journal
cases, 17 capacity cases, two actual CLI cases, formatting and all-target/
all-feature Clippy with warnings denied pass. The real Rust/server bridge also
refuses live-fixture route approval revocation and factory pause during later
operations, rejects cold sequence replay, and retains unknown failed-result
cost through canonical import. Its provider/controller/transport are controlled
fixtures: no model or real issue runs. Production execution remains unfinished.

Matched private `9aab13e` / public `eae15ab2` passes the complete assembly:
1,206 server cases with 38 existing skips, all 17 mock cases, production
server/shared-web builds and the input/artifact/credential/transport guards.
Log: `/home/khing/.cache/parasaur-enterprise-current.wv3hzg7j/build.log`.

The optimized `eae15ab2` CLI compares all 611 Rust/Cargo inputs against its
immutable archive and passes release build, context/spend smokes and the real
WSL host/journal/crash check with default capacity settings. Binary SHA-256:
`de6892083610b3ddde64019f8ae34dadfd400edf65f845eae668f1f927aebb93`.
The staged candidate is `artifacts/parasaur-native-context-eae15ab2/` (glibc
2.39); build log:
`/home/khing/.cache/parasaur-native-context-release.1gke6c1w/build.log`.

That exact optimized binary and the newly compiled matching private API pass
the real Rust/HTTP bridge, including both initial consumption and later
operation checks through the real owner-auth middleware. Concrete route
approval revocation and factory pause deny permission; cold replay cannot
reuse a sequence. The bridge's initial accounting assertion was corrected to
include its two new fixture approval-event rows in the pre-import baseline;
canonical receipt import still adds exactly one terminal result. No model or
real issue runs, and no installed CLI, API, worker or controller is replaced.

This remains source-only orchestration. The broker, concrete initial authority
check and conditional HTTP grant consumer exist, but runtime startup does not
construct a native broker/worker transport or executor. The consumer route is
mounted only on central when an owned broker is explicitly supplied. There is
no permissive authorization fallback. Actual-host reservations, native worker
execution, per-attempt/publication authority and live startup/reconciliation
must still be implemented and verified before real native work can run.

The broker joins the existing context and claim boundaries with private
`<root>/<run UUID>/broker.json` journals. It validates the prepared manifest,
request, policy and full binding rather than accepting caller-supplied paths.
Only an explicitly selected backend and non-default model may enter this path.
Stage and start intents are recorded before their corresponding transport calls.
A network deadline aborts observation only; it never proves execution stopped.
Lost replies, worker absence, expiry, restart and stale ready observations do
not repeat either operation. A missing journal alongside held ownership refuses
recreation. Concurrent calls for one run serialize; completed promises leave the
in-memory map, while run identity remains on disk.

An idle host reservation and a fresh, exact, at-most-ten-second start grant are
required. Expired/mismatched/denied grants cannot launch and release only the
idle reservation. Grants are passed to the transport and never stored in the
journal or returned view. They authorize the verified wrapper's initial start,
not subsequent backend attempts or publication. Production must implement those
later authority checks too.

Terminal observations prevent any later start. Receipts are privately spooled
before canonical import, with bounded input and at most four candidates per
run. Failed import retains the receipt and native ownership. A stale running
reply cannot postpone importing an already saved terminal receipt. Only the
canonical importer acknowledgement can release the exact matching ownership.
The spool hash protects original bytes; Rust's receipt digest identifies parsed
source JSON serialized canonically. These hashes need not be equal.

History chunks are reclaimed only after canonical acknowledgement. Cleanup
uses fixed, private, owned regular filenames, never recursive deletion. Failed
cleanup remains visible and terminal reconciliation retries it without starting
or importing again. Compact request, policy, binding, receipt and journal remain
for replay. Persistence failures latch the service until inspected/reloaded;
post-rename failures never restore an assumed previous state.

Disk journals reuse the existing atomic writer and claim service rather than
introducing a separate queue/database or retaining all run history in memory.
The deployment contract remains one central service writer per claims/journal
root. Operator deletion of durable history is not a supported replay mechanism.

## Current central policy prerequisite

`gah native-handoff check-current-policy` rereads the central configuration and
live ledger. It verifies the prepared context identity and policy hashes,
requires central role and enabled factory execution, compares current portable
policy/runtime restrictions and repository-policy bytes against preparation,
and runs the existing streaming spend admission against current history.
Spend recorded after preparation, including unknown actual cost under an
enforced limit, can therefore hold the job. It reuses spend admission's existing
deduplicated hold notification behavior.

The command returns the bound run and `policy_current: true` alongside
`execution_authorized: false`. This is deliberately a prerequisite rather than
an execution grant. The staged broker/authority pair combines it with current
work/route evidence, the live loop, exact claim and reservation identity.
Actual host reservation ownership and fresh authority for later attempts or
publication remain worker/executor requirements. Nothing is installed by this
change.

The trusted server wrapper `runCheckNativePolicy` uses fixed arguments and
bounded output, verifies the returned context/node, and supports aborting the
private CLI check. The context directory must be coordinator-owned; neither the
command nor arbitrary caller-selected paths are exposed as a generic remote
operation. Capability tests keep all native context commands local-only and
security-sensitive.

## Verification on November 6

### One-use initial grants

The broker now owns grant creation. Its mandatory authority callback returns a
fresh decision rather than inventing a bearer token. After affirmative checks
and the exact held claim, the broker creates 256 random bits, bounds expiry by
ten seconds and the host reservation, and durably records only the token hash,
reservation and expiry beside the start intent before transport.

`consumeStartGrant` rechecks current authority and the exact claim/context,
then durably records consumption before returning permission. Concurrent
consumers, cold replay, forged fields, expired grants, policy/loop revocation
and claim replacement cannot produce another permission. An uncertain commit
never returns permission, even when the consumed marker reached disk. A lost
reply burns the grant: a wrapper must stop/reconcile rather than infer approval
from a replay. No hold is released by grant expiry or consumption. Fresh running
observations without recorded consumption remain unconfirmed. The returned
view contains consumption time, never the token or token hash; consumption is
not evidence that a backend started or exited.

Consumption intentionally does not wait for the offer/start lock: the worker's
start request may be waiting on this callback. A bounded in-flight consumer set
and a synchronous re-read/commit after asynchronous authority checks prevent
double permission without that reentrant deadlock. Entries leave the set after
success, refusal or timeout; durable state remains in the existing per-run
journal rather than an unbounded in-memory grant map.

`nativeDispatchAuthority` checks a managed, active controller instance before
and after the actual current-work CLI check. Stopping/starting/missing units
refuse permission; an unreadable or timed-out bus remains unavailable rather
than being reported as a known pause. Both asynchronous systemd queries and
the existing synchronous loop-status query are bounded to two seconds/16 KiB.
One current-work snapshot runs at a time across profiles to bound transient
memory. Abort, timeout and failure clear that busy state; no permission cache or
snapshot queue is retained.

The conditional `POST /api/native-dispatch/consume` requires an owner/service
principal and current worker/profile registration. It accepts only six grant
fields, no paths or dispatch arguments. Registration is checked again after
the asynchronous policy check; revocation can burn a grant but cannot deliver
permission. The exchange is rate limited, returns no token and cannot issue a
new grant. Default startup still has no broker to mount it; this is an internal
connection point for the forthcoming worker transport, not a live feature.

These grants authorize only the verified wrapper's initial transition into
dispatch. Later attempts/publication require their own fresh authority; this
increment does not implement or bypass those checks, actual host reservation
ownership, worker process lifetime or production transport.

Windows passes all 25 broker/HTTP cases. Concrete authority checks pass on WSL,
including unavailable systemd, controller replacement, missing current-work
evidence and bounded/cancelled concurrent snapshots. The real Rust/server bridge
also passes with the optimized `8e5d08c5` CLI and the concrete authority callback,
using controlled controller/provider/worker fixtures. It consumes a broker-minted
grant during the start callback (without deadlock), loses the start reply,
reloads, refuses replay and imports one failed result with cost unknown. Plain
grant bytes do not appear in the journal. No model or real issue executes.

Final matched private `bf75f1a` / public `aa8f7ad8` passes 1,199 server cases
(38 existing skips), all 17 mock cases, production server/shared-web builds,
and release/input/credential/transport guards. Log:
`/home/khing/.cache/parasaur-enterprise-current.qhp6l2o3/build.log`.
Its compiled grant router and actual owner-auth middleware also pass the
disposable real Rust/server bridge over HTTP. Permission is consumed while the
start call is pending, then cold replay is refused. The API build identifies
both exact revisions; controller/provider/worker inputs remain fixtures.
Rust inputs are unchanged from optimized candidate `8e5d08c5`, so it was reused
without another binary build. Live API, controller and installed CLIs remain
untouched. The production worker executor/transport and startup wiring remain
unfinished.

### Current work and route revalidation

The private `check-current-policy --check-work` path now reads current central
ledger/provider evidence and asks the existing controller for its next action.
It does not filter out other work, active claims, incomplete evidence, higher
priority lifecycle work, backpressure or budget gates to make a request fit.
The prepared work identity, purpose, target, branch and selected review/repair
generation must match. Numeric issue targets `1` and `#1` use the existing
canonical identity; repository-relative ticket targets retain their paths.
Review, implementation, retry, escalation, repair and PM intents are distinct.
Provider-only merge/publication actions cannot authorize a native wrapper.

The exact backend/account/model is also checked through the existing route
engine using current ledger approvals and routing state. Standard controller
restrictions remain in force: a prepared request cannot add an MR override,
production/validation/intake overrides or change the controller retry shape.
PM's separate zero-retry shape is retained. PM source-depth and publication
contracts still require their normal checks during execution.

The server wrapper's `checkWork: true` requires affirmative `work_current`
evidence; a policy-only response cannot satisfy it. Both CLI forms continue to
return `execution_authorized: false`. This is still source-only: no live
broker caller, start grant issuer/consumer, worker transport/executor or
per-attempt/publication authority has been installed. The future controller
handoff must occur before acquiring a competing legacy/local claim. Ownership,
loop state and host reservation still need their separate live checks.

The actual CLI fixture found that legacy intake ignored `exec:human-required`.
The shared intake disposition now rejects it in legacy and canonical modes,
including alongside `exec:autonomous` and with a canonical-label override.
Discovery/direct-fetch dispositions and the fallback scan share that rule and
report `human_required` rather than silently selecting the issue.

Verification: 18 native context cases, 61 intake cases and 91 controller decision
cases pass; all-target/all-feature Clippy with warnings denied passes. The
actual CLI fixture first accepts a fake autonomous issue, then rejects it after
the human-required label appears; its backend canary is never invoked and its
forge log shows no mutations by either check. The existing context CLI case
also passes. The server flag/response test and server typecheck pass.

Public `8e5d08c5` / private `ebe1e9b` passes the complete matched enterprise
assembly: 1,186 server cases, 38 existing skips, 17 mock cases, production
server/shared-web builds and release/input/credential/transport guards. Log:
`/home/khing/.cache/parasaur-enterprise-current.182uz12i/build.log`.

The immutable optimized `8e5d08c5` CLI compares all 608 Rust/Cargo inputs against
its source archive and passes release/context/zero-spend smokes. Its SHA-256 is
`73578a153e7f05c1cb8bbf18316928a3b4c5c2fccb73a0fcfe38e71436a2679a`;
the staged directory is `artifacts/parasaur-native-context-8e5d08c5/`.
The exact binary also passes the real Rust/server broker bridge with controlled
provider and worker fixtures. Current controller/route checks are exercised
inside its authorization callback. A human-required label added after context
preparation refuses the run. Revoking the exact paid-route grant after
preparation also refuses it, and restoring the grant permits the check again.
These approval operations change only the disposable fixture ledger. No real
provider writes or backend/model executions occur. Lost-start/reload recovery,
single canonical import, failed outcome/unknown-cost preservation and history
cleanup still pass. The grant generator/transport remain controlled fixtures,
so this does not prove production native execution.

A read-only 20:00 UTC observation confirms the same controller `666974` and
observer `667504` remain running: 543 samples, 32,806.617 seconds (about 9.1
hours), no sampling gaps and no OOM kills. This is still short of the ten-hour
observation requirement and does not supply sustained agent-load acceptance or
prove the leak fixed. No live service or installed CLI was changed.

- Server `npm run --workspace=apps/server typecheck` passes.
- Windows: 37 of 39 combined broker/claim cases pass; two existing POSIX-only
  claim cases skip. All 15 broker cases pass.
- Ubuntu-24.04 WSL: all 39 combined cases pass, including all 15 broker cases.
  Cases cover concurrency, lost stage/start replies, deadlines, reload, stale
  state, failed imports, mismatched acknowledgements, missing journals, deferred
  cleanup and injected post-rename journal/receipt persistence failures.
- A disposable WSL bridge uses optimized Rust candidate `cd87332e` for actual
  context preparation, worker context checking and canonical import through
  `runReceiveNativeResult`. Its controlled transport loses the start reply,
  reloads the broker, then supplies a failed-process receipt. Stage, start and
  import each occur once; failure and unknown cost remain intact; the saved
  canonical digest matches the acknowledgement; ownership releases only after
  import; history is reclaimed; terminal replay starts/imports nothing again.
  The canary backend and real issues are never invoked.
- Matched private commit `cf6e538` pins public `c32ad26f`: its complete assembly
  passes 1,185 server cases (38 existing skips), all 17 mock cases, production
  server/shared-web builds, release/input guards and credential-route/transport
  checks. The verification log is
  `/home/khing/.cache/parasaur-enterprise-current.c382n0vp/build.log`.
- A read-only observation at 19:15 UTC confirms the original controller
  `666974` and observer `667504` are still running: 499 samples, 30,165.928
  elapsed seconds, zero observation gaps and zero OOM kills. This is about
  8.4 hours, short of the ten-hour observation requirement and not sustained
  agent-load acceptance. It does not prove the memory issue fixed.
- Current-policy increment: all 16 native context cases pass, including new
  configuration/permissions/identity checks and both measured exhausted spend
  and unknown spend arriving after preparation. Nine capability unit cases,
  12 capability integration cases and the actual native-context CLI case pass;
  formatting and all-target/all-feature Clippy with warnings denied pass.
  Server typechecking and all 15 broker cases pass. The disposable real
  CLI/server bridge additionally refuses a central pause and changed runtime
  policy, then retains unknown imported cost as a live budget hold. The
  transport/grant generator remains controlled test code, not production
  authority. No backend or real issue runs.
- Optimized Rust candidate `2ddcf9db` compares all 607 Rust/Cargo source inputs
  against its immutable source archive. Release build, context checks and
  zero-spend/current-policy refusal smokes pass. The exact optimized binary
  also passes the real CLI/server broker bridge. SHA-256:
  `fed503c7c8b2188ec0be7f00080226737fbbe464a44e6b3400269c0f380a7551`.
  It is staged at `artifacts/parasaur-native-context-2ddcf9db/`, not installed.
  Public `94ef7b38` changes only a server test relative to that candidate;
  its Rust/Cargo inputs are identical.
- Initial private `34ecfc1` / public `2ddcf9db` assembly stopped on the ACP
  archive test's PID-presence assertion (1,184 passed, one failed, 38 skips).
  Twenty diagnostic isolated archive runs passed and observed absent fixture
  processes. The test's `kill(pid, 0)` predicate could count an already-exited
  Linux zombie as alive, unlike the existing production termination contract.
  Public `94ef7b38` corrects the test to distinguish executable processes from
  exited zombies, preserving errors when process state cannot be read. The
  corrected targeted archive test passes. This changes test interpretation,
  not production cleanup, and does not prove the reported memory issue fixed.
- Final matched private `e5b7610` / public `94ef7b38` assembly passes all
  1,185 server cases (38 existing skips), all 17 mock cases, production
  server/shared-web builds, release/input guards and credential-route/transport
  checks. Log:
  `/home/khing/.cache/parasaur-enterprise-current.cmur17qb/build.log`.
  This supersedes the initial failed assembly for release verification, while
  retaining that failure above. No live API, controller or installed CLI was
  changed by the current-policy increment.

Local logs: `artifacts/native-broker-windows-tests.log`,
`artifacts/native-broker-linux-tests.log` and
`artifacts/central-live-20261105/native-broker-bridge.json`. The bridge is run
with `artifacts/probe-native-broker.mjs` after
`node artifacts/build-native-claim-runtime.mjs`; it takes the staged Rust
binary as its sole argument. These are local validation artifacts, not a live
dispatch or beta-completion claim.
