# Durable compute start and result delivery — 2026-11-04

Verified private `a58ce10d7fe9d321bd8a2077ec32e442d85c0dbe` against public
core `8963ebb5cb218e0a8d4802c6f8c22987870624be`. The native core is unchanged
from the [shared capacity and supervision checks](enterprise-host-supervision.md).

One exact compute claim now has a durable local execution transaction. Start
intent is flushed before requesting cloud start authorization. A lost start
reply cannot release the job as an unstarted deferral or retry its program.
Cleanup must be verified before saving the result; result delivery can then
retry independently. The node-bound status read resolves lost delivery replies
without renewing the lease or approving execution.

The journal lives outside job mounts, stores only identities and result
receipts, and rejects unsafe ownership, links and malformed records. A partial
record remains unresolved while unrelated recorded results can still advance.
Incomplete executions are counted for scope reconciliation; they are not
automatically rerun or declared successfully cleaned up.

## Verification

- Fresh combined production build: **1,041 server cases passed**, 26 explicit
  skips, zero failures; **13 mock cases passed**. Server/web typechecks,
  production builds, artifact/input guards and transformed transport checks
  passed. Six additional opt-in execution cases account for the increased skip
  count; they ran in the dedicated WSL command below.
- `npm run test:isolation`: **38 passed**, one platform-only skip, zero
  failures. This includes the earlier native boundary checks plus durable
  phases, duplicate claims, mount separation, corrupt record isolation,
  lost start/result/release replies and abrupt connector termination.
- An actual SIGKILL during execution left the start intent durable, stopped the
  owned scope and refused a second acquisition of the same claim. This is
  process-crash evidence, not a physical power-loss test.
- Disposable migration replay and SQL authorization checks passed, including
  cross-node status denial, superseded claims and revoked devices.
- Real Auth/PostgREST checks deliberately discarded successful committed RPC
  replies. Lost start: zero program attempts and a failed remote outcome. Lost
  completion: one program attempt and a completed outcome recovered from the
  receipt. Reopening the journal and retrying the claim did not execute it again.
- Real sandbox-to-inference, concurrent inference retries, revocation,
  HTTP/WebSocket relay and membership/entitlement checks passed.
- Seven startup crash offsets (0, 2, 5, 10, 20, 40 and 80 ms) again left no
  unauthorized execution or active scope.

Evidence is retained under
`/home/khing/.cache/parasaur-enterprise-current.tw1vndx4/`: `source`,
`build.log`, `isolation-verification.log`, `auth-verification.log`,
`startup-crash-matrix.json` and `native-host/manifest.json`.
The matching retained debug native CLI has SHA-256
`3398247d8fc803f1dc5ede0e11d639677c54c889348e98c5797984fc34bf188b`.
Its Rust/Cargo source still matches the pinned public core; no new Rust build
or installed CLI upgrade was needed for this private integration.

## Still required

Automatic compute queue polling, post-crash scope reconciliation, journal
retention, bounded writable workspace storage, production dispatch/client and
forge runtime, portable bindings, ledger integration, staged migration and
complete simultaneous local/cloud acceptance remain unfinished.

The tests used local logical devices, a prepared Python image and an HTTP
fixture provider. No live subscription, full factory repository workflow,
remote node, hosted deployment or release installation occurred. Owned test
scopes and disposable service processes were cleared; the existing worker,
installed desktop and installed CLI were unchanged.
