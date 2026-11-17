# Verified crash reconciliation and scope cleanup — 2026-11-04

Verified private `ac04a1a4a6dd6d81465beae43d2ec358becae2c4` against public
core `2cc5e3fd99ce3a4cc2165694712d014a3345b67d`. This supersedes the
[queued compute pair](enterprise-compute-queue.md).

Recovery now uses durable machine/boot/PID-namespace/start-time identities.
It refuses live, foreign or unreadable ownership. A registered broker must
have exited before recovery can stop its exact delegated scope and prove the
kernel cgroup empty. Interrupted attempts are settled without rerunning them
or inventing a backend exit code. Empty storage can then be reclaimed; saved
archives, nonempty partials and unfamiliar data remain preserved.

## Reproduced problems and repairs

- The broker could launch before its PID record became durable. Native protocol
  2 now emits `ready` and waits for `g`; registration is flushed before that
  byte. Older binaries reject the required version flag. No lease or scope is
  created before the handshake, and cloud authorization remains a later gate.
- Systemd nests `parasaur-jobs.slice` under `parasaur.slice`. The old kernel
  cleanup path missed that parent. Both native supervision and reconciliation
  now inspect the complete path. A regression attaches an additional process
  to the actual scope and requires its death before cleanup returns.
- A startup crash sometimes left an active but empty scope until its runtime
  deadline. Inspection found zero kernel processes and no broker; execution
  remained unauthorized. Bounded manager housekeeping now retires that empty
  unit after independent kernel cleanup. Missing cgroup-v2 state stays unknown.
- The broad run exposed a chat fixture race: socket open did not prove the
  observer's hello had been processed. It now waits for subscription replay;
  the busy-before-chunk ordering assertion remains intact.

## Exact-pair verification

- Fresh combined build: **1,060 server cases passed**, 37 explicit skips,
  zero failures; **13 mock cases passed**. Server/web typechecks, production
  builds, artifact/input guards and transformed transport checks passed.
- `npm run test:isolation`: **68 passed**, one platform-only skip, zero
  failures. The dedicated command includes the new recovery file.
- Actual connector SIGKILL during registration created no lease/scope or
  unauthorized job; recovery released it as unstarted work. SIGKILL during
  execution stopped its scope and produced an `interrupt` receipt with no
  invented exit code. Live/legacy owners and live brokers were refused.
- A real orphan scope was stopped only after its registered broker exited.
  Cross-process empty-slot recovery removed known zero-byte unpublished
  partials and preserved saved output. Metadata deletion pins the directory
  inode so slot reuse cannot redirect it into a new job's files.
- Real disposable Auth/PostgREST killed an automatic consumer after its HTTP
  fixture-provider call, restarted it and verified **one provider attempt**,
  a terminal failed receipt with **unknown exit**, and recovered empty storage.
- Two normal queued compute/inference/archive jobs, SQL replay, lost committed
  start/result replies, revocation, membership and HTTP/WebSocket relay checks
  also passed. Interruption cannot overwrite measured terminal results or act
  through a revoked/foreign device.
- All seven supplemental startup offsets (0, 2, 5, 10, 20, 40 and 80 ms) left
  inactive scopes and no unauthorized marker. Twelve repetitions of the
  formerly failing 40 ms window were clean on the retained final binary. A
  tracked case additionally covers six timings around the launch handshake.
- Rust formatting, Clippy across all targets/features with warnings denied,
  11 CLI capability/parser cases and 32 source-structure cases passed. The
  corrected chat integration file passed all nine cases before the clean
  combined build.

Evidence is retained under
`/home/khing/.cache/parasaur-enterprise-current.oirdds26/`: `source`,
`build.log`, `isolation-verification.log`, `auth-verification.log`,
`startup-crash-matrix.json`, `startup-scope-linger-probe.json` and
`native-host/manifest.json`. The retained matching debug CLI has SHA-256
`ac62e3cc98730f2e8839d7d79b867933e342b95b18a5f3f9cfd0708edaa1cf2e`.
The manifest records normalized Rust/Cargo source agreement. Owned test scopes
and disposable service processes were absent after verification.

## Remaining acceptance

Unknown/legacy ownership remains reserved for explicit reconciliation. Archive
review/retention controls, journal and orphan-capability metadata maintenance,
production dispatch/client and scoped forge runtime, portable bindings,
execution identity/ledger integration, staged migration and full simultaneous
control-plane acceptance remain unfinished. Kernel reboot behavior is inferred
from identity rules; physical power loss and fresh-OS acceptance were not tested.
In-memory edits not yet archived can still be lost on abrupt job death.

These are logical devices and a Python/HTTP fixture workflow on one Windows
machine's WSL distro. No live subscription, full production repository workflow,
hosted deployment or release installation occurred. The installed desktop,
user CLI and worker release were not altered; this debug binary is development
evidence and requires the matching protocol-2 connector.
