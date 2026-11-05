# Current enterprise integration — 2026-11-05

The current assembled pair is private `3679931` with public core `a6a38c4d`.
It also includes [streaming event history](event-reader-memory.md), preserving
full controller recovery history while bounding limited event-query retention.
It includes the [bounded Rust capture repair](rust-output-memory.md): fixed-size
live output retention, coalesced progress and rejection of incomplete reviewer
evidence. This closes a reproduced path; #1681's actual Linux OOM allocation
source and long-running stability still need live profiling.
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

The exact production pair passed 1,093 server cases (37 explicit skips), 15 mock
cases, typechecks, production builds and artifact/transport guards. The preceding
receipt-recovery pair retains 19 shared-output browser cases, a full sample
Settings workflow and inspected desktop/mobile layouts. Its server/web/SQL
production source is unchanged; this increment changes Rust runner code and
the private core pin. The admission-view pair retains its five cloud Settings
browser cases, real SQL/Auth/PostgREST admission reporting/expiry, queue/crash
recovery, maintenance, authorization and HTTP/WebSocket relay evidence.
The preceding `9b4ae6d` Auth driver passed SQL replay and real Auth/PostgREST,
preserving local 125 and cloud completed/0 during retired-claim denial with no
extra provider attempt. Reviewed cleanup kept the other job's archive and receipt.
Queue, crash/recovery, retry-receipt and authorization/relay checks passed.
Earlier Windows desktop checks passed 35 ordinary cases and the separate hidden
native output fixture. The older `ac62e3` native binary has the seven startup
offsets and twelve repeated startup-window checks. The refreshed debug host is
`20368c67`; its isolation-host sources are unchanged. That earlier timing-stress
record is historical, not a claim of a new repetition on this binary.

The preceding `e7d088c` / `2f8c9cf5` pair and `fe07bdd2` host passed 96 dedicated
WSL isolation cases (one platform skip) after the Ubuntu-24.04 restart. The
complete log is retained alongside the assembly described in the memory record.

Live Linux OOM attribution/soak, legacy/unknown state resolution, unavailable/revoked delivery authorization,
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
