# Shared host capacity and independent supervision — 2026-11-04

Verified public core `8963ebb5cb218e0a8d4802c6f8c22987870624be` with private
`d28710bafd21a1d87355033f7e0757ed7d403c1f`. This supersedes the current-pair
startup evidence while retaining its authorization and inference repairs.

The local-only native host reserves capacity through the controller's existing
flock registry and supervises the job independently of Node. Heartbeat expiry
stops the owned cgroup. Cancellation closes the control pipe first; uncertain
cleanup returns a bounded error while the guardian retains its reservation.
See the [protocol](../../ISOLATION_HOST_PROTOCOL.md).

Trusted sandbox initialization closes inherited host descriptors before running
untrusted code and applies the job environment only after authorization. These
checks reproduced descriptor leakage through the default PID 1 and premature
loader-environment execution before their repairs.

## Exact-pair evidence

- Combined production build: 1,027 server cases passed, 20 explicit skips,
  13 mock cases passed; server/web typechecks, builds and artifact/transport
  guards passed.
- Dedicated WSL isolation run: 18 passed, one platform-only skip. Coverage
  includes namespaces, limits, authorization, cancellation, parent death,
  descriptor closure, loader environment, inference socket and lease renewal.
- Actual Node suspension caused native heartbeat expiry in about 10.4 seconds;
  resuming Node did not restart execution.
- Shared admission deferred a second reservation, then admitted it after the
  first released capacity. This used an explicit debug 64 GiB pressure model
  and 40/20 GiB reservations; it did not allocate those physical amounts or
  prove simultaneous control planes on two machines.
- One isolation case deliberately used an unresponsive fixture guardian holding
  a real flock and a scope refusing manual stop. The connector returned an
  error without killing the guardian or releasing capacity. Restoring the
  fixture cleared both; this was not an observed kernel I/O failure.
- Seven startup crash offsets (0, 2, 5, 10, 20, 40 and 80 ms) passed without
  unauthorized execution or remaining scopes.
- Disposable SQL replay and real Auth/PostgREST/HTTP fixture-provider checks
  passed, including sandbox-to-inference flow, concurrent retries, revocation,
  relay cancellation and workspace membership removal.
- Rust: Clippy across all targets/features, 24 capacity cases, 11 CLI capability
  cases and 32 source-structure cases passed. The 2,499-case library run passed
  2,498 initially; its installer case passed after correcting the test helper's
  npm PATH. Source-structure checks required Git metadata in the owned cache.

Retained evidence is under
`/home/khing/.cache/parasaur-enterprise-current.b4xjxokj/`: `source`,
`build.log`, `isolation-verification.log`, `auth-verification.log`,
`startup-crash-matrix.json` and `native-host/manifest.json`.
The retained debug CLI at `native-host/gah` has SHA-256
`3398247d8fc803f1dc5ede0e11d639677c54c889348e98c5797984fc34bf188b`.
Rust/Cargo source comparisons against the public core had no mismatches.

## Remaining acceptance

The compute queue consumer, durable execution journal/reconciliation,
production provider/client runtime, scoped forge access, portable aliases,
execution identity/ledger integration, bounded writable workspace storage,
configuration migration and full simultaneous control-plane acceptance remain.
Capacity admission does not impose a CPU quota or bound writable disk usage.

No live provider subscription, full queued factory job, remote node or hosted
deployment was tested. Test scopes and disposable service processes were gone
after verification. The existing worker was not changed. Installed CLI,
desktop and public recovery package remain at their earlier versions; this
retained debug binary is development evidence, not an installed release.
