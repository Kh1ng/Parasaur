# Current enterprise integration — 2026-11-04

The current verified pair is private `d28710ba` with public core `8963ebb5`.
It retains the earlier authorization and inference repairs and adds
[shared host capacity and independent native supervision](enterprise-host-supervision.md).
A frozen Node controller cannot renew the native heartbeat; cancellation keeps
capacity reserved until cleanup is proved, including bounded error handoff when
cleanup remains uncertain.

The exact pair passed 1,027 server cases (20 explicit skips), 13 mock cases,
typechecks, production builds and artifact/transport guards. Dedicated WSL
isolation checks passed 18 cases with one platform-only skip, including an
intentionally unresponsive guardian fixture. Seven startup crash timings and
real disposable Auth/PostgREST-to-HTTP-provider, retry, revocation and relay
checks passed. See the supervision record for fixture limits and Rust results.

The compute queue consumer, durable execution journal, production runtime/client,
forge access, portable aliases, bounded workspace storage, execution identity,
configuration migration and complete shared-hardware acceptance remain unfinished.
These results are local development evidence; installed releases and live services
were not upgraded.

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
