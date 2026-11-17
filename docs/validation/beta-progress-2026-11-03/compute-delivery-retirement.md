# Retired-claim delivery reconciliation — 2026-11-05

Verified source candidate: private `9b4ae6d6a960497ed2e600c5fca2731c8fd3ddbd` with
public core `b0cd55e1b6f1495c1f13ce0e1ac4265abc0eacf6`.

## Reproduced blockage and repair

A local result of 125 can coexist with an immutable cloud completed/0 result
when a retired claim is presented again and denied at its start gate. The old
delivery loop kept trying to finish that claim, could not overwrite its cloud
result and left the local receipt pending indefinitely. The regression fails
against the preceding committed production sources and passes after this repair.

The connector now records retired separately from delivered. Exact result
acknowledgement still confirms delivery. A mismatched terminal or superseded
observation instead requires fresh strict-true proof that the exact workspace,
node, work and claim cannot execute again. It records an immutable retirement
disposition while preserving both the original local result and cloud outcome.
It never renews a lease, starts a program or calls a provider to reconcile it.

Superseded alone is insufficient: changing devices can produce that status even
while the same claim is active. Missing work, denied credentials, malformed or
false proof, unavailable services and cancellation remain pending. An execution
without verified cleanup and a durable result remains unresolved.

Reopen skips settled retired receipts and still rejects duplicate local claims.
A failed filesystem write cannot report retirement as settled. Owner review can
then select an unreferenced retired receipt, visibly labeled delivery unconfirmed;
actual removal again requires fresh cloud proof and all existing ownership,
stopped-broker and storage checks. Partial removal checkpoints preserve either
disposition. Saved output remains available through separate controls.

This uses the existing retirement-proof migration; there is no new production
SQL migration. Older journal readers reject the new disposition as unreadable
and preserve it. Owner catalog/review fields are additive and optional for old
nodes. The original local exit code is never replaced by the cloud exit code.

## Evidence

Retained assembled source:
`/home/khing/.cache/parasaur-enterprise-current.7fm8lho8/source`.
The production build and native/browser checks ran at private `c013752`.
Private `9b4ae6d` changes only one Auth acceptance assertion to include the new
zero retired counter. The verification checkout was fast-forwarded for that
driver; its production sources and built artifacts are unchanged. The release
manifest continues to identify its actual build input, `c013752`.

- **1,093 server cases passed**, 37 explicit skips, zero failures; **15 mock
  cases passed**. Server/web typechecks, production builds, private input/artifact
  guards and transformed browser transport checks passed.
- **19 browser cases passed**: 18 behavior cases and the desktop/390px render
  and focus case. Retired output stays downloadable and cleanup says delivery
  unconfirmed. All four final captures were inspected.
- **One full sample Settings workflow passed**, exercising independent receipt
  cleanup, output removal, reset and refresh through the actual proxy.
- Dedicated native isolation: **96 passed**, one platform-only skip, zero
  failures. These include filesystem failure before settlement, restart,
  unchanged local results, strict retirement proof, fresh maintenance proof and
  partial cleanup with either disposition.
- Corrected-driver real Auth/PostgREST/edge and SQL replay checks passed. An
  actual retired-claim replay was denied locally with 125 while cloud completed/0
  remained unchanged, with no extra provider call or archive. Fresh-proof owner
  cleanup removed only that retired receipt; the other job's receipt and archive
  remained. Queue execution, lost start/result replies, consumer crash/recovery,
  inference retry receipts, authorization and HTTP/WebSocket relay checks passed.
- Focused verification passed **54 server cases** with eight explicit native
  skips and 18 browser behavior cases at
  `/home/khing/.cache/parasaur-retirement-review.3nwwby1i`. Initial UI/render
  evidence and the failing old-source regression are retained at
  `/home/khing/.cache/parasaur-retirement-review.907xy1jd`.
- Disposable SQL replay passed at
  `/home/khing/.cache/parasaur-compute-db.b9s1mpr_`, including the added active
  claim/device reassignment case. No live database was used.

Logs beside the assembled source are `build.log`,
`output-browser-verification.log`, `output-settings-verification.log` and
`isolation-verification.log` and `auth-verification.log`. The final user-manager
check found no remaining `parasaur-job-*.scope` units.
The retained native host's SHA-256 remains
`ac62e3cc98730f2e8839d7d79b867933e342b95b18a5f3f9cfd0708edaa1cf2e`;
its CLI Rust/Cargo sources were matched to the public pin.

## Remaining work

Revoked/unavailable authorization and unreadable legacy receipts still need
owner intervention; the connector cannot manufacture retirement proof. Orphan
capability metadata, actual hosted deployment, Michael's live review gates,
installed desktop acceptance, production dispatch/client and forge access,
updates and physical iOS acceptance remain unfinished. MacBook work is deferred.
No installed application, live factory, provider account or cloud service was
changed by these fixtures.
