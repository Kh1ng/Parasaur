# Server output retention regression (#1680)

The Mac report remains unverified without host access. A separate, reproducible
server retention bug was confirmed against local source at `5f0dbb11`.

`SessionManager` accumulated every stdout/stderr chunk in arrays until explicit
stop or one-hour cleanup. Repository and private-overlay searches found no
caller of its output-history accessor. This duplicated the browser's bounded
replay and the CLI's existing artifact logging. `runDispatchCancellable` also
accumulated stderr without a limit, and terminal session errors could retain
that entire string. A separate cancellation-ID set had no removal path for
already-running sessions.

The fix removes the unused server transcript/accessor, retains live forwarding,
and ignores output for missing or terminal sessions. Diagnostic strings retain
their last 16,384 UTF-16 code units plus a visible omission notice; copied tails
cannot retain a larger source string's backing allocation. Dispatch exit codes,
stdout streaming, request idempotency, queue ordering, and confirmed process-tree
cancellation remain covered. Queue cancellation now uses the existing session
state, including absence after cleanup, instead of permanent ID tombstones.

## Reproduction

`apps/server/src/sessions/outputRetention.test.ts` launches an isolated Node
process with `--expose-gc` and fake dispatch/provider dependencies. It streams
10,000 unique random chunks (40,960,000 characters) across stdout/stderr into a
discarding push subscriber, then compares retained JS heap after GC. No live
provider, factory job, production profiler, or production GC is involved.

- Before: the regression failed with 41,274,168 retained heap bytes.
- After: a Windows run delivered all characters and retained 52,840 heap bytes;
  the running session was not cancelled. The assertion permits 8 MiB of runtime
  variance, far below the previous full-transcript retention.

These are isolated JS-heap measurements, not Mac RSS or physical-footprint
measurements. They establish the retention fault and its regression, not the
cause of the user's observed Mac process growth. Durable CLI logging and the
browser replay limits are unchanged; a diagnostic tail is intentionally not a
complete transcript.

## Verification

Focused Windows session/lifecycle/memory checks passed (14 tests). The Linux
CLI/session group passed all 45 tests before the additional cancellation-state
cleanup; it included a real fake CLI emitting oversized stderr, with the last
cause, omission marker, stdout and nonzero exit status all preserved. Root
typechecking passed after rebuilding the shared contracts (a direct server-only
typecheck initially saw stale generated contract declarations).

Final source passed all-package typechecking, server production build, 995
server tests (one opt-in skip), and twelve mock tests. Its Linux heap regression
delivered 40,960,000 characters with 43,376 bytes of post-GC retained heap growth.
The broad server validation is logged at
`/tmp/parasaur-memory-fix-validation.log`. Deployment and comparable Mac idle/
normal-workflow memory observations remain required before closing #1680.
