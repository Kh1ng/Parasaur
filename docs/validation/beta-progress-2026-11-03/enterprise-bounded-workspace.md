# Bounded job workspace and archive — 2026-11-04

Verified private `637f088f3f489511a125a61e043b7371bbbc9026` against public
core `8963ebb5cb218e0a8d4802c6f8c22987870624be`. It retains the native
supervision and [durable compute transaction](enterprise-compute-recovery.md).

The new bounded mode mounts host input read-only, copies it into a size-limited
memory filesystem after authorization, and exports edits as an opaque archive.
The host accepts one byte-limited upload over a job-specific Unix socket. The
job receives no writable host directory or output-file descriptor. Remaining
descendants are stopped and reaped before the snapshot. Failed export produces
an explicit nonzero result, rather than claiming the work was saved.

The archive is flushed and published without replacing another result; its
receipt records byte count and digest. It remains untrusted data and is not
extracted on the host. Original input is preserved. Abrupt job termination can
lose its in-memory edits; this mode does not claim crash-persistent dirty work.

## Exact-pair checks

- Fresh combined build: **1,048 server cases passed**, 31 explicit skips,
  zero failures; **13 mock cases passed**. Server/web typechecks, production
  builds, artifact/input guards and transformed transport checks passed.
- `npm run test:isolation`: **50 passed**, one platform-only skip, zero
  failures. Five new kernel checks cover retained edits, actual workspace
  `ENOSPC`, oversized archive failure, denied authorization and detached
  descendants. Seven archive-service checks cover bounded publication,
  declared/chunked overflow, empty/aborted input, private output and conflicts.
- The workspace fixture uses a 16 MiB tmpfs inside a 512 MiB job cgroup.
  An archive exceeding its 10 KiB allowance failed with exit 122. Successful
  controlled tar members were read for inspection; no archive was extracted.
- Real disposable Auth/PostgREST checks now use bounded storage for the durable
  compute flow. Losing a committed start reply produced no program attempt or
  archive. Losing a committed completion reply retained one archived attempt,
  with unchanged host input and receipt-based delivery recovery.
- SQL replay, sandbox-to-inference, concurrent inference retries, revocation,
  HTTP/WebSocket relay and workspace membership checks passed.
- Seven startup crash offsets (0, 2, 5, 10, 20, 40 and 80 ms) again passed
  against the current isolation implementation. These supplemental offsets
  exercise the original workspace mode; the dedicated suite covers bounded
  mode separately.

Evidence is retained under
`/home/khing/.cache/parasaur-enterprise-current.pj5b1czr/`: `source`,
`build.log`, `isolation-verification.log`, `auth-verification.log`,
`startup-crash-matrix.json` and `native-host/manifest.json`.
The unchanged matching debug native CLI has SHA-256
`3398247d8fc803f1dc5ede0e11d639677c54c889348e98c5797984fc34bf188b`.
No Rust implementation changed in this pass. Owned test scopes and disposable
service processes were absent after verification.

## Remaining integration

Automatic queue startup still requires a node-wide retained-artifact budget,
journal retention and post-crash scope reconciliation. A per-job output cap
does not bound all saved jobs. Existing explicit callers retain the original
direct-workspace mode; the automatic consumer must select bounded mode.

Production dispatch/provider and forge runtime, safe artifact review/publication,
portable bindings, execution identity/ledger integration, staged migration and
complete simultaneous control-plane acceptance remain unfinished. The test
runtime contains Python and uses local logical devices and an HTTP fixture
provider. No full repository workflow, live subscription, remote node or hosted
deployment was established. Installed CLI, desktop and existing worker remain
unchanged; these private library changes were committed locally.
