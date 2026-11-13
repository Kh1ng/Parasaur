# Workspace compute admission visibility — 2026-11-04

Private `4bca434a6103977e2662df93ebeeaddb19813e5a` pairs with public core
`f11b53c3b7b7c68c34c087e7c6f6dba848972c68`. Workspace machines now distinguishes
worker liveness, dashboard connectivity and compute admission. A live connector
can explain a full receipt journal, retained-output storage, configured capacity,
disabled/stopped compute or an unavailable admission check.

## Behavior and boundaries

The compute consumer updates a fixed admission state at its actual gates.
The existing outbound heartbeat carries that state, never work IDs, local paths,
archive/receipt metadata, output or raw errors. Hardware-sharing members can read
the status under the existing node visibility policy without dashboard access.
Owner maintenance permissions are unchanged. Guidance directs the machine owner
to the separate [receipt/output controls](journal-retention-controls.md).

Liveness cannot imply readiness. A stalled compute loop's last local check turns
unknown after 60 seconds; cloud admission observations independently expire
after 90 seconds using server time. Bare heartbeats do not refresh that evidence.
Polling means local admission checks passed, while job requirements and cloud
capacity still govern whether work can run.

The existing heartbeat's 10-second write bound remains in force. During a rolling
upgrade a rejected admission report can fall back once to a bare heartbeat within
the same five-second deadline. Denied credentials are not retried. An older
dashboard RPC supplies liveness only. Workspace/session changes discard pending
health replies; an unavailable read clears the observation to unknown.

The private migration `20261104000005_compute_admission_health.sql` and updated
`node-health` edge handler are required for hosted reporting. Neither has been
deployed. The existing three-column health RPC remains compatible.

## Verification

The committed pair's full build is retained at
`/home/khing/.cache/parasaur-enterprise-current.estm6b_f/source`:

- **1,087 server cases passed**, 37 explicit skips, zero failures; **15 mock
  cases passed**. Server/web typechecks, production builds, private input/artifact
  guards and transformed browser transport checks passed.
- Dedicated native isolation: **90 passed**, one platform-only skip, zero
  failures, retaining queued execution, receipt/output maintenance, crash recovery
  and bounded sandbox/archive checks.
- Final-pair SQL replay and real Auth/PostgREST/edge checks passed. They prove
  a device-bound receipt-limit report is visible, a stale admission report becomes
  unknown while node liveness remains online, and outsider reads stay empty.
  Queue-to-sandbox/inference/archive, receipt maintenance and retired-claim denial,
  connector crash/recovery, retry receipts, entitlement/revocation and HTTP/WS
  relay checks also passed with synthetic loopback providers.

Its retained native host has SHA-256
`ac62e3cc98730f2e8839d7d79b867933e342b95b18a5f3f9cfd0708edaa1cf2e`;
CLI Rust/Cargo sources were matched to this public pin before retaining it.
Sibling logs are `build.log`, `isolation-verification.log` and
`auth-verification.log`. The final user-manager check found no remaining
`parasaur-job-*.scope` units.

Focused source retained at
`/home/khing/.cache/parasaur-admission-review.gisro67x`:

- Server and web typechecks passed.
- **11 focused server cases passed**, two explicit native-isolation skips.
  These cover receipt/storage gate states, disabled configuration, maintenance
  resuming polling, failed checks recovering, stopped consumers, heartbeat field
  restrictions, stalled-loop expiry and old-edge fallback without retrying 401.
- **Five browser cases passed** against the actual CloudHosting component and
  intercepted cloud transport: hardware-only visibility, recovery, older services,
  failed reads, workspace-switch races, fixed status vocabulary and desktop/mobile
  layout. Both final captures were inspected. The initial race test had an
  overly exact select-label locator; the corrected test waits for the late reply.
- Disposable SQL migration replay and authorization passed at
  `/home/khing/.cache/parasaur-compute-db._zwwsdma`. It checks rate bounds,
  credential/node binding, independent expiry, private-node isolation, hardware
  sharing, outsider denial and revocation.
- The Deno edge-handler test passed, including invalid states and attempts to
  attach identity or private metadata.

The focused worker, health, contract and browser files were compared with the
committed assembled pair and match after line-ending normalization.

## Remaining work

This reports the opt-in enterprise queued-compute consumer. It does not establish
the cause of Michael's live review gates or imply deployment to his machine.
Actual hosted acceptance, pending-delivery reconciliation, legacy/unknown
receipts, orphan capability metadata, production dispatch/client and forge access,
installed updates and physical iOS acceptance remain unfinished. MacBook work
remains deferred. No live provider, factory or installed application was changed.
