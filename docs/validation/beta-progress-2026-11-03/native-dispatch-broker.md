# Durable native dispatch broker

This is source-only orchestration. `NativeDispatchBroker` has no production
constructor, HTTP route, permissive authorization fallback or worker executor.
Live services and installed CLIs were not changed. Fresh central policy/budget
authorization, actual-host reservations and the worker execution transport must
still be implemented and verified before it can dispatch real work.

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

An idle host reservation and a fresh, exact, at-most-30-second start grant are
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
an execution grant. The production authority must additionally check the live
loop, exact claim, current work/review eligibility, account approvals and worker
reservation, and repeat the appropriate checks before subsequent attempts or
publication. No such production authority/executor is installed by this change.

The trusted server wrapper `runCheckNativePolicy` uses fixed arguments and
bounded output, verifies the returned context/node, and supports aborting the
private CLI check. The context directory must be coordinator-owned; neither the
command nor arbitrary caller-selected paths are exposed as a generic remote
operation. Capability tests keep all native context commands local-only and
security-sensitive.

## Verification on November 6

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
