# Current enterprise integration — 2026-11-04

The current verified pair is private `a58ce10d` with public core `8963ebb5`.
It retains [shared capacity and independent supervision](enterprise-host-supervision.md)
and adds [durable compute start/result delivery](enterprise-compute-recovery.md).
A lost start reply cannot trigger a second program attempt; a lost completion
reply can be reconciled from its receipt. Interrupted records remain unresolved
without blocking delivery for unrelated completed claims.

The exact pair passed 1,041 server cases (26 explicit skips), 13 mock cases,
typechecks, production builds and artifact/transport guards. The dedicated WSL
run passed 38 cases with one platform-only skip. Seven startup crash timings
and real disposable Auth/PostgREST checks passed, including lost committed
start/completion replies and the sandbox-to-inference fixture flow. See the
recovery and supervision records for fixture limits and earlier Rust results.

The automatic compute consumer, post-crash scope reconciliation, journal
retention, production runtime/client, forge access, portable aliases, bounded
workspace storage, execution identity, configuration migration and complete
shared-hardware acceptance remain unfinished. Installed releases and live
services were not upgraded.

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
