# Current enterprise integration — 2026-11-04

The private workspace build was pinned behind the finishing branch and its API
client conflicted with current account/cancellation changes. The private source
now targets public core `bd377e21`, retains the current model/account/factory
recovery work, and incorporates the deployment-prefix WebSocket URL helper.
Final overlay preflight found no conflicts. The public submodule pointer now
selects validated private revision `d46e6793`.

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
