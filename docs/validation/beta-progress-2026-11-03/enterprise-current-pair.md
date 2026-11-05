# Current enterprise integration — 2026-11-05

The current source pair is private `9b4ae6d` with public core `b0cd55e1`.
Its production build input is private `c013752`; the revision difference is one
Auth acceptance counter assertion, with identical production sources/artifacts.
It retains automatic queued compute, bounded archives and durable delivery,
with [owner/scope reconciliation](enterprise-compute-reconciliation.md) and
[saved-output review controls](saved-output-controls.md) in shared Settings,
now extended with [receipt retention controls](journal-retention-controls.md).
The [workspace admission view](compute-admission-visibility.md) separates worker
liveness from disabled, paused, capacity-limited and polling compute. Hardware
members can see fixed recovery guidance without private dashboard access; stale
compute observations expire independently of liveness.
Owners can inspect the independent journal limit and confirm a bounded cleanup
of settled, unreferenced receipts with fresh cloud retirement proof. Partial
removal preserves durable lookup evidence and can resume after a new review.
The [retired-claim delivery repair](compute-delivery-retirement.md) also settles
otherwise endless pending delivery using independent exact-claim proof, while
keeping local results and cloud outcomes distinct. Retired receipts stay visibly
delivery-unconfirmed and require fresh proof again for owner-reviewed cleanup.
The [Windows native output check](windows-native-output.md) adds real filesystem
streaming, corruption and cancellation evidence without a new production bridge.
Protocol 2 blocks launch until broker registration is durable. Full kernel
slice-path checks and bounded housekeeping close the reproduced cleanup gaps.

The production pair passed 1,093 server cases (37 explicit skips), 15 mock cases,
typechecks, production builds and artifact/transport guards. Dedicated WSL
isolation passed 96 cases with one platform-only skip. Nineteen shared-output
browser cases and a full sample Settings workflow passed; desktop/mobile layouts
were inspected. The preceding admission-view pair retains its five cloud Settings
browser cases, real SQL/Auth/PostgREST admission reporting/expiry, queue/crash
recovery, maintenance, authorization and HTTP/WebSocket relay evidence.
The corrected current Auth driver passed SQL replay and real Auth/PostgREST,
preserving local 125 and cloud completed/0 during retired-claim denial with no
extra provider attempt. Reviewed cleanup kept the other job's archive and receipt.
Queue, crash/recovery, retry-receipt and authorization/relay checks passed.
Earlier Windows desktop checks passed 35 ordinary cases and the separate hidden
native output fixture. The unchanged retained native binary also has the earlier
pair's seven startup offsets and twelve clean repetitions of the failing window.

Legacy/unknown state resolution, unavailable/revoked delivery authorization,
capability-metadata maintenance,
installed Tauri large-file saving, production dispatch/client and forge runtime,
portable aliases, ledger integration, configuration migration and complete
shared-hardware acceptance remain unfinished. Installed releases and live
services were not upgraded or enabled for cloud compute.

## Earlier integration record

The private workspace build was pinned behind the finishing branch and its API
client conflicted with current account/cancellation changes. The private source
then targeted public core `bd377e21`, retained the model/account/factory
recovery work, and incorporates the deployment-prefix WebSocket URL helper.
That overlay preflight found no conflicts. The public submodule pointer at that
point selected validated private revision `d46e6793`.

The integration work also reproduced and corrected cloud request cancellation
before admission and preservation of caller abort signals. Late replies cannot
restart canceled reads. RPC retry budgets now bound the network call itself.
The stronger cancellation/deadline regressions failed before their respective
repairs and pass afterward.

The final pair passed the complete combined production build: **1,003 server
tests passed, one opt-in skip, no failures**, plus **12 mock tests**, server/web
typechecks, production builds, private artifact/input guards and transformed
browser transport checks. The release manifest records the exact public and
private revisions. These private artifacts remain local.

Disposable migration replay, real Auth/JWT/edge/PostgREST authorization,
invitation/removal, node enrollment/sharing and HTTP/WebSocket relay checks also
passed. Windows headless Chromium verified the two-account sign-in, invitation,
acceptance, member controls and removal workflow against the real disposable
account services. Desktop/mobile captures were inspected. Fixture processes
were stopped and the temporary Windows session file removed.

Detailed private evidence remains under the private checkout's ignored local
validation records. No private source, credentials or fixture sessions belong
in the public artifact. No push, hosted deployment, live workspace mutation or
real provider job occurred. Hosted acceptance and shared-hardware execution
isolation remain pending; these local checks do not establish beta readiness.
