# Bounded result replay and enrollment repair — 2026-11-05

Validated private revision `3bfc79b908bdc17b607efc6739515737ed77057a`
with public core `819937bf8c7e2b5530bf195cdd1e1015bf755726`.

Completed work could remain stuck behind slow or denied result-delivery RPCs.
Replay now shares a two-second network budget, stops further delivery requests
after the first RPC failure, and rotates past the last attempted exact claim on
the next pass. It still scans receipt metadata for accurate pending/unresolved
counts. Filesystem I/O is outside that network budget. The cursor is process-local
and resets on connector restart.

The connector reloads its private enrollment each iteration, so a same-origin
credential repair applies to subsequent claims and delivery without restarting.
Changing the origin pauses admission before any request to the new target.
Already admitted jobs and their inference relays retain their validated credential
snapshot; ongoing lease and revocation checks remain in force.

Denied delivery produces fixed authorization-repair guidance; other RPC failures
produce fixed connectivity/configuration guidance. Neither raw errors nor private
credentials appear in those lifecycle reports. Durable local results remain held.
Replay never reruns a program, invents success, or bypasses exact-claim delivery
and independent retirement proof. Local journal-write failures do not classify
the cloud as unavailable or stop unrelated receipt delivery.

## Verification

- 36 targeted execution, worker and relay checks passed, including denied
  delivery, shared deadline, fair rotation, same-origin repair without a program
  rerun, cross-origin refusal and credential-snapshot binding.
- The exact assembled pair passed 1,099 server tests (38 explicit skips), 15
  mock tests, server/web typechecks, production builds, private input/artifact
  guards and transformed browser transport checks.
- Dedicated native WSL isolation checks passed 100 cases, with one platform
  skip and no failures, using retained native host hash `f6028d54`.
- Disposable SQL migration replay and real Auth/JWT/edge/PostgREST checks passed,
  including queue-to-sandbox inference/archive, lost replies, crash recovery,
  receipt maintenance, retired-claim execution denial, recipient isolation,
  removal, and HTTP/WebSocket authorization and relay regressions.

The credential-repair regression uses controlled HTTP responses. The real Auth
suite verifies the changed relay interface and existing claim/authorization
boundaries; it does not establish hosted credential-rotation acceptance.

Retained WSL evidence is under
`/home/khing/.cache/parasaur-enterprise-current.qsubplov/`: `build.log`,
`isolation-verification.log` and `auth-verification.log`. Targeted evidence is
`/home/khing/.cache/parasaur-delivery-recovery.zl0b0q67/checks.log`.
After WSL recovery, those terminal logs were read again and no owned
`parasaur-job-*.scope` units remained.

## Remaining acceptance

This adds local lifecycle guidance, not a per-receipt authorization UI or new
cloud state. Legacy/unbound receipts, workspace reassignment, cross-origin
migration, capability-metadata maintenance and hosted provider acceptance remain
open. A newly enrolled identity cannot automatically prove an older claim settled.
There are no schema/migration, public runtime, Rust, CLI or desktop changes in
this increment. Existing installed binaries and the public recovery candidate
remain unchanged. Nothing was pushed or deployed; no real provider work ran.
