# Automatic queued compute and persistent archive admission — 2026-11-04

Verified private `afd4cee424a6aaf82d245ad5f6b55915e31a25eb` against public
core `8963ebb5cb218e0a8d4802c6f8c22987870624be`. It retains the
[bounded workspace/archive](enterprise-bounded-workspace.md), durable start
intent and independent native supervision.

Node startup now has an opt-in compute consumer with explicit owner-private
runtime, profile, workspace and resource configuration. Claims select an
allowed profile; fixed owner argv runs in the sandbox, with job facts supplied
through fixed environment fields. The inference capability is job-specific.
Ambient host/provider credentials are not forwarded.

Persistent archive slots admit storage before the cloud claim RPC. They bound
retained archive payloads across processes and restarts. Saved output and
uncertain executions keep their slots; dirty output is never deleted to admit
another job. Full archive or journal capacity leaves new work queued and emits
an operator action in lifecycle logs. These reasons are not yet surfaced as
dedicated shared-web controls.

Shutdown cancels and joins owned claims, scopes, relays and writes. A late claim
reply cannot start a program after shutdown. Journal replay excludes active
claims and yields between batches so result recovery does not starve lease
heartbeats. Unfinished scope/slot reconciliation remains explicit work.

## Exact-pair evidence

- Fresh combined build: **1,058 server cases passed**, 33 explicit skips,
  zero failures; **13 mock cases passed**. Server/web typechecks, production
  builds, artifact/input guards and transformed transport checks passed.
- `npm run test:isolation`: **62 passed**, one platform-only skip, zero
  failures. New coverage includes persistent cross-process admission, private
  profile configuration, full-store/journal deferral before claim, concurrent
  jobs, late claim replies and shutdown during actual execution.
- Three separate processes raced to initialize/use one archive slot; exactly
  one reserved it. Process exit did not make the slot available to another.
  Conflicting policies and attempts to discard saved output were refused.
- Recovery over 100 retained records allowed an event-loop callback to run;
  active claims were excluded from unresolved counts.
- Real disposable Auth/PostgREST queued **two jobs** through the actual
  consumer, native sandbox, inference relay/worker, HTTP fixture provider,
  bounded archive and terminal cloud receipt. Each job made repeated inference
  reads but produced **one provider attempt**. Both remote outcomes completed
  with exit zero, separate archives and unchanged host input.
- SQL replay, lost committed start/completion replies, entitlement/member
  removal, revocation and HTTP/WebSocket relay checks passed.
- Seven supplemental startup crash offsets again passed against the exact
  source. Their probes use the original workspace mode; dedicated tests cover
  the bounded consumer separately.

Evidence is retained under
`/home/khing/.cache/parasaur-enterprise-current.c602gw75/`: `source`,
`build.log`, `isolation-verification.log`, `auth-verification.log`,
`startup-crash-matrix.json` and `native-host/manifest.json`.
The unchanged matching debug native CLI has SHA-256
`3398247d8fc803f1dc5ede0e11d639677c54c889348e98c5797984fc34bf188b`.
No Rust implementation changed. Owned scopes and disposable service processes
were absent after verification.

## Remaining acceptance

Automatic post-crash scope/slot reconciliation, journal/archive maintenance and
review controls, production dispatch/client and scoped forge runtime, portable
bindings, execution identity/ledger integration, staged migration and full
simultaneous control-plane acceptance remain unfinished. The journal threshold
is checked before claim; in-flight slot owners can add bounded extra records.
Archive limits do not impose a quota on unrelated host disk use or all metadata.

The queued programs use the prepared Python test image and an HTTP fixture
provider on local logical devices. This proves a queued compute/inference
transaction, not a complete production repository/factory workflow, real
subscription, two physical machines or hosted deployment. Whole-server startup
wiring compiled; the real-service checks started its consumer directly.
The feature is opt-in and was not enabled on the existing worker. Installed
desktop, installed CLI and recovery release remain unchanged.
