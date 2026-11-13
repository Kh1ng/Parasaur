# Welcome without a duplicate status query

`sendWelcomeMessage` awaited a full CLI status snapshot after lease
reconciliation. That coupled the connection handshake to ledger reads and
forge availability. Failure then sent empty work arrays, which could confuse
unavailable observations with an empty queue. Dashboard pages already fetch
their status through REST; the legacy WebSocket work fields are optional and
are not consumed by the current pages.

Public `6c473dd6` removes this duplicate status query and omits its work fields.
The welcome retains the provider catalog and reconciled sessions. Lease
reconciliation remains mandatory before advertising session state. The sender
rechecks that the socket is open and authorized after awaiting reconciliation.
The dashboard's REST loading, failure, retained-state and reconnect-refresh
paths remain authoritative. The navbar now treats a missing snapshot as
unavailable, including the initial pending read; it cannot advertise Idle
merely because a welcome arrived without worker observations.

Using a short timeout and sending empty arrays would preserve the data-truth
problem. Removing the redundant query avoids the dependency and keeps missing
observations absent. This does not remove the separate lease-reconciliation
network work or bound every REST status request.

## Verification

All 13 real-handler WebSocket tests pass. The new regression installs a
bounded slow/failing fake CLI and verifies welcome never invokes it. Optional
work fields remain absent. The same test fails at its CLI-query assertion on
the original implementation. Existing tests retain persisted-lease recovery,
fresh reconnects, authentication, worker restrictions, activity replay and
legacy-client capability negotiation. The isolated source check logs are in
ignored WSL `parasaur-welcome-check.o3g0c0kh` and
`parasaur-welcome-check.izaq6jga` directories.

All ten Kanban recovery and dashboard-freshness browser cases pass. The new
case holds REST status while sending a minimal welcome, verifies Loading and
Factory activity unavailable, then fails REST and verifies an error rather
than No jobs yet. Existing reconnect behavior still refetches REST data.
Server and web typechecks pass. The public notification-compatible backport
passes 35 Linux tests across startup, WebSocket, activity and preferences.

Private assembly `d3bfa05` pins `6c473dd6` and passes 1,111 server tests
(38 explicit skips), 16 mock cases, production server/web builds/typechecks,
private artifact/input guards and credential transport checks. Its log is
`/home/khing/.cache/parasaur-enterprise-current._91ptesd/build.log`.

## Live deployment

Shared web `6c473dd6` completed rollout at 2026-11-06T07:11:43Z. All 30 served
asset files match the immutable archive manifest. Bootstrap and existing old
asset chunks were preserved. No native UI automation was performed.

The API remains public base `6b101b98`, retaining notification/protocol
corrections through `0ce6e7fb` and startup correction `e362380f`, with the
welcome correction through `6c473dd6`. This rollout replaced only compiled
`wsServer.js`. The build applies the recorded notification prerequisite and
current source patches separately; its expected preimage matches the live
notification-compatible module. Source patches, artifact hashes and manifests
are in ignored `artifacts/parasaur-welcome-6c473dd6/`. The managed dependency
lock matches. Persistent-auth behavior remains unchanged.

A fresh preflight verified 100 chats, no active chats and an explicit empty
job roster. API rollout passed at 07:13:13Z, leaving PID 539448. Chat index,
node identity, config, owner credential, legacy handoff and all 262 real
notifications in the verification window were preserved. Controller PID
437234 was not restarted and no memory policy was lowered.

The read-only live Tailnet TLS check at 07:16:09Z completed an initial
connection and reconnect. Both received a provider catalog and reconciled
session list with all unobserved work fields absent. No events or paid work
were injected. Health subsequently reports healthy. Deployment and live
reports use the `websocket-welcome-` prefix in ignored local
`artifacts/central-live-20261105/` and the existing VM repair evidence directory.

Physical capacity and complete factory progression remain open. In particular,
the controller-activity and last-decision REST paths still request broad event
windows; measure their payload and memory cost before changing retention or
query semantics. Persistent GitLab sign-in, provider billing reconciliation
and Mac memory acceptance also remain open. Rebuild the old staged auth API
from current source before rollout so none of these server fixes regress.
