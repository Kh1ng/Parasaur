# Inspect a remote worker's factory — 2026-11-05

The fleet roster carried remote execution and claim observations, but discarded
work-item blockers. The ordinary controller decision view read the connected
controller's events. It could not explain another worker's queue without access
to that worker's shell.

Nodes now has an explicit **Worker factory state** check for a registered worker
and one of its declared profiles. Central reads that worker's existing status,
controller-status and last-decision endpoints. The result distinguishes each
source independently, showing profile gates, work awaiting action, bounded next
steps, controller state and the last decision in the worker's 24-hour event window.
The controller's running state is not presented as proof of agent execution.

This uses a new GET behind the existing API authentication boundary (including
its trusted same-origin loopback exemption):
`/api/registry/nodes/:nodeId/factory?profile=...`. It is read-only and no-store.
There is no generic URL proxy. The node must remain the same registration while
the check runs, and the profile must be declared by it. Credentials resolve on
central and are sent only to the registered origin under the existing transport
rules. Redirects are refused. One five-second network/body deadline covers all
three concurrent reads, each limited to 2 MiB. This bounds the central request;
the worker's own CLI execution remains governed by its existing handlers.

Only selected public fields are returned. Each gate list shows at most 50 items
while retaining the reported total; explanations/steps are bounded and only three
step summaries are included. Commands, API mutation descriptions and arbitrary
extra response fields are omitted. Unknown, unauthorized, unsupported, oversized,
malformed and unreachable sources do not become empty gates or an idle controller.
Failed refreshes retain a labeled previous check. Changing profile clears it and
late replies cannot restore the previous profile's data.

The alternative was to attach remote blockers to the central queue and reuse its
remediation controls. An explicit source-bound read avoids silently acting on the
wrong ledger. The new panel deliberately offers no retry, release, assignment or
controller mutation. Existing health/capacity/claims and automatic polling are
unchanged.

## Local validation

- Six real HTTP tests passed on Windows: fixed read endpoints, credential/header
  forwarding, bounded projection and omitted canaries, partial failures, wrong
  profiles, oversized/malformed bodies, a body that never finishes, redirects,
  changed registration, declared-profile enforcement, and the actual central
  route's local/auth boundary and no-store response.
- Eleven Windows component checks passed across the new panel and Nodes page.
  The strengthened three-panel subset also passed after waiting for the actual
  late response before asserting profile isolation.
- Server/web typechecks and the production web build passed. Desktop and 390px
  screenshots were inspected; the panel fits without horizontal overflow.
- The complete browser Nodes workflow passed against the stateful sample
  endpoint, without response stubs for factory data. Its driver was added after
  the assembled core pin; production and sample endpoint sources are identical.
- All 16 sample-control-plane checks and its typecheck passed. The sample worker
  observation remains in memory and does not operate the central controller.
- Screenshots remain under `apps/web/test-results/component/` in the
  `NodeFactoryDetails-worker--b5d59-work-gate-without-mutations` result directory.

These controlled workers do not establish access to Michael's machine, explain
the current live PR gates, or establish hosted cloud/installed desktop acceptance.
The existing public recovery ZIP predates this feature. No live service, claim,
provider login or controller state was changed, and nothing was pushed.

## Exact assembled integration

Private `3cac46c50c4cec70a4d2c165929755ef2ff284b2` pins public core
`26ed70e83696ba56b2f9cd8f646ac1e62be076a7`. The final combined build passed
1,105 server cases (38 explicit skips), 16 mock cases, server/web typechecks,
production builds and private input/artifact/transport guards. The assembled
reader and UI match the reviewed public sources, and the panel is present in
the emitted private dashboard.

Fresh disposable SQL migration replay and real Auth/JWT/edge/PostgREST and
HTTP/WebSocket relay regressions passed, retaining queue/sandbox/inference,
crash recovery, lost replies, receipt maintenance, recipient isolation and
removal checks. This is generic Auth/relay regression evidence. The route-specific
central-to-worker auth/transport behavior is covered by the six controlled HTTP
tests; no hosted cloud worker-factory check is claimed.

Evidence: `/home/khing/.cache/parasaur-enterprise-current.3ielflxe/`, including
`build.log`, `auth-verification.log`, and `source/.build/release.json`.
The earlier assembly attempts stopped on the newly added test's stricter Linux
JSON typing, then missing sample-route coverage. Both were corrected and the
final build passed. Dedicated native isolation was not repeated; its Rust
implementation is unchanged. No installed executable or release ZIP was updated
in this increment, and live access/acceptance remains open.
