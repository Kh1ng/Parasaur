# Account activity evidence — 2026-11-05

The subscription activity marker credited a claim or running controller action
through the latest ledger backend. It also treated `Session.instanceId` as an
account, although that field identifies provider transport, and chose the first
backend match when several accounts used the same provider. A remote session
could therefore light a local account with the same name. Four new Windows unit
cases reproduced the old behavior before the repair.

The account projection now retains the exact backend/instance scopes represented
by each subscription. Activity uses `Session.backendInstance` for a running
session or a fresh `RunningWorker.backend_instance` observation. Provider
transport IDs, process names, model guesses, claims and controller intent are
no longer inputs. Duplicate/ambiguous account mappings remain unconfirmed.
Pool instances can credit their containing account once without selecting the
first sibling account.

Tagged observations must match the local node; a tagged observation with unknown
local node identity cannot credit an account. Untagged records retain their
local-source interpretation. Worker state, valid activity time and the reported
stale interval must all agree. Starting sessions and stale, expired, future-time
or invalid-time worker observations do not prove current execution. App ignores
retained session activity when the WebSocket disconnects and ignores the worker
roster after an unavailable status read. Activity is recomputed on app renders
so freshness is reevaluated alongside the existing refreshes.

The marker means a matching execution was observed, not that an issue is making
progress or incurring a measured charge. An unlit marker does not prove idle;
older records without account identity can no longer guess an account. Quota
read failure continues to label the readings unconfirmed independently.

Keeping the old fallback but removing claims was considered. That would leave
transport, sibling-account and node crossover intact. Using the execution facts
already present on the wire removes those guesses in one shared projection.
No API, ledger schema, provider or dependency changed.

Verification: 17 Windows account-cost/activity/Kanban unit cases passed, as did
web typechecking and production build. Five Playwright cases passed, including
three full UI workflows: activity transitions across claim, starting session,
exact local account, remote session, fresh worker and stale worker; quota failure
and held retry; subscription popover/cards. The other two cases cover window
formatting and account/pool aggregation. The production cost and quota behavior
remains unchanged. Logs are retained at `artifacts/account-activity-*.log`.

The matched `6b101b98` public package and `8ec34f7` private assembly now include
this change. The private build passed 1,106 server cases (38 skips), 16 mock
cases and artifact/transport guards. Its exact assembled UI passed 20 Windows
component and five browser cases, including the activity transitions above.
The public build passed 1,011 server cases (two skips), 16 mock cases and
isolated release installation; all 29 JavaScript chunks served byte-for-byte.
The existing WSL user CLI is byte-identical to the new candidate and both native
Windows read-only integration checks passed again. No replacement was needed.
Native desktop/settings and all six worker-release hashes remain unchanged.
Logs: `artifacts/account-activity-private-*.log`; package/install records are in
`artifacts/parasaur-central-6b101b98/`. Authenticated live acceptance remains.
No real account session or live factory was mutated by these checks.
