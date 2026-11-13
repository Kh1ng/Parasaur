# Beta finishing pass: local validation

Native fix/review jobs now hold provider-visible issue claims with the existing
assignee/comment protocol, renewing under fresh central authority on a journal
separate from backend execution. Real controller acceptance passes normal
renewal, central pause and takeover by another identity through the matched API
and a debug CLI; lost claim authority fails the job without redispatch while
validation stays separately recorded. In-job journal opens now wait briefly for
busy observation locks. See [provider claims](native-provider-claims.md).

Real controller acceptance now passes normal dispatch and both lost reservation
and activation replies through the matched API and optimized worker CLI. It found
and fixed a catalog serialization refusal and a dormant-reservation intake stall.
Each fixture executes/imports once, proves capacity released, survives reload and
does not redispatch completed work. Production rollout and provider/PM lifecycle
integration remain pending. See [controller acceptance](native-controller-routing.md#real-controller-acceptance-and-recovery).

Current controller source adds worker account discovery, central route planning,
durable dormant preparation and explicit ownership transfer before activation.
Canonical native holds now inform scheduling and fresh grant authority. Real HTTP
tests cover transfer and lost replies; production rollout and complete end-to-end
acceptance remain pending. See [controller native dispatch](native-controller-routing.md).

Latest native release: the worker verifies central's durable exact-receipt
acknowledgement, persists stop intent and proves its recorded kernel lease is
free before confirming release. Original context history chunks are then cleaned;
working ledger/session evidence remains. Cold retry recovers a real stop reply
timeout without issuing another stop. Compact status distinguishes confirmed
Released from unknown. Browser/mismatched/pending acknowledgements cannot release
capacity. Seven WSL process cases cover the lifecycle with controlled authority/
backend/forge; production central API wiring remains unfinished. See
[acknowledged capacity release](native-worker-host-admission.md#acknowledged-capacity-release).

Latest terminal recovery: private `native-handoff result` verifies the recorded
job's actual exit and empty cgroup, streams past the exact original history and
spools one immutable actual ledger receipt. The original row is retained without
normalization. Cold replay survives later ledger corruption or unit collection;
receipt corruption and live jobs/descendants are refused. The real Rust receiver
records once and acknowledges identical replays without releasing worker capacity.
Forty-six native cases, 573 dispatch cases, 76 provider cases, three CLI cases,
12 capability cases, formatting and Clippy pass. Six controlled-authority WSL
process cases and the independent descendant/crash test cover recovery/import.
Acknowledgement-driven release and central API wiring remain unfinished;
nothing is installed. See [terminal result recovery](native-worker-host-admission.md#terminal-result-recovery).

Latest native launch: private Linux `native-handoff start` now fences the offer,
starts the fixed executable, records its actual invocation/PID/start ticks and
releases the journal lock before PID-verified credential delivery. Five real
WSL job cases cover execution, intake/publication denial, discarded launch replies
and cold replay refusal, and unsupported provider claims. The separate real
crash/descendant capacity test also passes. Journal version 3 retains legacy
fences and consumed identity. Central-to-worker API wiring and terminal result/
claim/capacity reconciliation remain unfinished. No live service is changed.
See [fixed launch transport](native-worker-host-admission.md#fixed-launch-transport).

Latest fixed worker: private Linux `native-handoff execute` receives bounded
credential stdin, detaches it before children, rechecks staged history without
copying it, consumes the start and installs actual process authority. A held
permission no longer becomes a phantom backend launch failure in fix/PM usage.
Real WSL jobs with controlled authority/backend fixtures cover allowed attempt,
paused intake and denied publication, preserve capacity after exit and reject
replay. Provider-assignee claim mode is refused before consumption until its
renewal lifecycle is integrated. Forty-one native cases, 573 dispatch cases,
76 provider cases, three CLI cases, 12 capability cases, formatting and Clippy
pass. Launch transport, provider claims and terminal result/release reconciliation
remain unfinished; nothing is installed. See
[fixed worker executable](native-worker-host-admission.md#fixed-worker-executable).

Latest dispatch guards: verified process authority now gates backend attempts
and fix/review/PM publication. Main attempt counters are behind the permission
check, changed route identity cannot overwrite ledger attribution, and uncertain
completion preserves actual exit/usage evidence while preventing further work.
Profile env loading cannot redirect native history. Thirty-eight native cases,
573 dispatch cases, 76 provider cases, two actual CLI cases, formatting and
all-target/all-feature Clippy pass in Windows WSL. These are source/fixture
checks; no production wrapper installs the authority yet and no live service
was changed. See [process-scoped guards](native-worker-host-admission.md#process-scoped-dispatch-guards).

Latest worker consumer: initial consumption and operation intent are now
durably fenced before bounded, stdin-credential control requests. Replies must
match the full scope and validity window, and the job's actual service/PID/
start ticks and holder are checked. Explicit completion is required before
another operation. Thirty-three native cases, eight HTTP-helper cases, two CLI
cases and Clippy pass; the real WSL process check also rejects changed/dead
job identities while retaining capacity. Entry records in that process check
are controlled fixtures. Wrapper launch and backend/publication hooks remain
unfinished; no model or installed service runs this increment. See
[worker permission consumption](native-worker-host-admission.md#worker-side-permission-consumption).

Latest continuation: the staged broker renews current controller/policy/spend
and exact-route prerequisites per attempt/publication sequence after consumed
start authority. Permissions are short-lived, durably sequenced and never
replayed; current owner/worker membership and exact claim/context are required.
Thirty-one broker/HTTP cases, four concrete-authority cases, Rust checks and a
real CLI/server bridge pass. Target/generation validation, the actual wrapper,
attempt/publication workflow and production transport remain unfinished. See
[continuation prerequisites](native-dispatch-broker.md#sequenced-continuation-prerequisites).
Matched private `9aab13e` / public `eae15ab2` passes 1,206 server cases (38
existing skips), 17 mock cases, production builds and release/credential guards.
The optimized CLI verifies 611 source inputs and passes the real WSL crash
check and matching compiled HTTP consumption/operation/result bridge. These
remain staged candidates; no live services or installed CLIs were changed.

Latest worker recovery: host admission now commits a private, per-run journal
under a cross-process file lock. Initial start offers are fenced durably before
transport and cannot repeat after lost replies, reopen or uncertain writes.
The compiled Windows WSL recovery inspector requires the actual matching holder
before reporting Ready; missing evidence stays unknown. Twenty-five native
context/host/journal cases, 17 capacity cases, two CLI cases, Clippy and the
expanded real process check pass. This remains staged; the verified wrapper,
result/release lifecycle and production transport still need implementation.
See [worker start fences](native-worker-host-admission.md#durable-worker-start-fences-and-recovery-inspection).
Optimized `09504779` verifies 611 source inputs and passes the real WSL
host/journal/crash check plus the real Rust/compiled HTTP grant/result bridge.
Unchanged server/contracts source permits reuse of the prior compiled API;
this is not a newly built enterprise assembly or installed worker rollout.

The original central memory observer has now completed ten hours on the same
controller with no restarts or OOM kills. Its 597 samples include one delayed
344-second interval, despite the report's empty explicit-gap list, and cgroup
charge reached the existing cap. This is not sustained agent-load acceptance
or a leak-fixed claim. See
[completed observation](review-preflight-budget.md#completed-ten-hour-observation-with-a-cadence-limitation).

Latest host admission: the private native command reserves actual worker
capacity under the existing memory/CPU guards and transfers its lock to a
separate systemd holder. The real Windows WSL check preserves capacity through
a disposable job's main-process crash and full stop, without an agent or a
resident reservation process. Context/capacity/CLI tests, formatting and Clippy
pass. This is staged; production transport, durable worker execution/recovery
and later authority remain required. See
[native worker host admission](native-worker-host-admission.md).

Latest initial grants: broker-minted tokens are private, expire within ten
seconds and can be consumed once after fresh controller/work/claim checks.
Lost replies, restart, expiry and uncertain disk commits never authorize a
second start or release ownership. A conditional internal HTTP exchange checks
service ownership and worker/profile registration, including revocation during
the check. Windows broker/HTTP cases and WSL concrete-authority checks pass;
the real Rust/server bridge also passes with controlled runtime fixtures.
Nothing is installed. Live startup, host admission transport, the worker executor and
later attempt/publication authority remain unfinished. See
[initial grants](native-dispatch-broker.md#one-use-initial-grants).
Matched private `bf75f1a` / public `aa8f7ad8` passes 1,199 server cases (38
existing skips), 17 mock cases, production builds and artifact/credential guards.
The compiled HTTP consumer and real owner-auth middleware also pass the real
Rust/server bridge with controlled runtime inputs. No services or installed
CLIs were changed; the unchanged optimized Rust candidate was reused.

Latest work revalidation: the private native check now re-observes the current
controller action and exact route before accepting a queued intent. Changed
work purpose, targets, review generations, incomplete evidence and unavailable
or unapproved routes cannot be accepted through a policy-only response. An
actual CLI fixture also exposed and now verifies the shared intake correction
for `exec:human-required`, including legacy mode. Context/intake/decision checks,
the CLI fixture, server wrapper checks and Clippy pass. This remains staged;
the live broker caller, grant lifecycle and worker executor are unfinished.
See [current work revalidation](native-dispatch-broker.md#current-work-and-route-revalidation).
Matched private `ebe1e9b` / public `8e5d08c5` passes 1,186 server cases (38
existing skips), all 17 mock cases and its production/release guards. The exact
optimized CLI also passes current controller/route and paid-grant revocation
checks through the real Rust/server bridge, with controlled provider/worker
fixtures. Nothing was installed or restarted. The original memory observation
is still running at about 9.1 hours; no leak-fixed claim is supported.

Latest current-policy prerequisite: the private native CLI/server path checks
current central policy, permissions, factory enablement and live spend after
context preparation. Changed restrictions and newly recorded unknown/exhausted
spend refuse admission. Sixteen context cases, capability/actual CLI checks,
Clippy, server typechecking and the actual CLI/server bridge pass. It explicitly
does not grant execution; live work/account/claim/host authority and the worker
executor remain unfinished. See [current policy checks](native-dispatch-broker.md#current-central-policy-prerequisite).
Matched private `e5b7610` / public `94ef7b38` passes 1,185 server cases (38
existing skips), 17 mock cases, production builds and release/credential guards.
The optimized `2ddcf9db` CLI has identical current Rust inputs and passes the
real policy/context/import bridge. It is staged, not installed. An ACP test's
Linux zombie/PID-presence ambiguity was corrected; no production cleanup or
memory-fix claim follows from that test correction.

Latest broker: [durable native orchestration](native-dispatch-broker.md) records
stage/start intent per run, retains results through failed canonical import and
refuses duplicate launch after lost replies or restart. Fifteen broker cases
pass on Windows and WSL; the combined claim/broker suite passes 37 Windows
cases (two POSIX skips) and all 39 Linux cases. An actual Rust preparation,
worker-check and canonical-import bridge passes with controlled transport.
Matched private `cf6e538` / public `c32ad26f` passes the full enterprise
assembly: 1,185 server cases (38 skips), 17 mock cases, production builds and
release/credential/transport guards.
This is source-only: production authority, host admission, worker execution
and live reconciliation remain unfinished. Usage and persistent GitLab auth
also remain unverified in the owner's actual client.

Latest native ownership: [durable holds and canonical acknowledgements](native-worker-ownership.md)
keep work reserved across expiry, disconnect, import failure and restart until
the exact native result is durably recorded. Unknown cancellation cannot release
ownership. An actual Rust/server bridge proves refused unconfirmed termination,
failed-result/unknown-cost preservation, single import and safe late replay.
Focused Windows/Linux checks, ledger/CLI checks, formatting and Clippy pass.
The immutable optimized `cd87332e` candidate passes that same import bridge,
and matched private `910d660` passes 1,170 server and 17 mock cases plus its
production builds and artifact/credential guards. Neither is installed.
This is source-only: fresh authority, actual-host admission and the durable
per-run broker/executor are still required before agent work can launch.

Latest production compatibility evidence: public `448bab97` prepares and
verifies the actual central history on Windows WSL for a pinned OpenCode/model
route. All 26,365 snapshot entries survive the two-chunk transfer and are read
through the worker's normal effective ledger path. Automatic routing still
refuses the worker's missing OpenHands profile. Fresh authority, host admission
and durable native execution remain unfinished. See
[pinned production acceptance](native-worker-context.md#pinned-route-production-acceptance).
Thirteen focused cases, the actual CLI check, 228 launcher cases, four explicit
route cases and all-target Clippy pass. Matched private `1593ebe` also passes
its complete assembly. The worker gained central's supported Claude safe-mode
setting; all other configuration is preserved. Services and installed CLIs
were not restarted/replaced; no model or real issue ran.

The owner's usage screenshot was also rechecked against current live data:
the tailnet HTML and all 30 assets match the corrected deployed build, and the
current selector keeps the expired Nous reading separate from Mistral and
historical. The particular running client and provider collection/billing
accuracy remain unverified; see [live quota evidence](provider-quota-freshness.md#live-recheck-after-the-owners-usage-report--november-6).

Latest native input: [policy/history context](native-worker-context.md) prepares
and verifies a job's central history, limits, validation and generation while
retaining worker-local credentials, paths and capacity settings. The actual CLI
check reads prepared central history despite an inherited empty worker ledger
and never starts a backend. It is source-only: authority refresh, execution
admission and the durable native broker remain unfinished.
Context, ledger, actual CLI, dispatch, capability, telemetry and spend checks
pass, including 570 dispatch cases, real daily/monthly budget rollover and
all-target/all-feature Clippy. Counts and overlapping filters are documented
in the linked evidence.
Matched private `5385b42` / public `6b67abe1` passes its full assembly. A clean
605-input optimized native build also passes production context and zero-spend
smokes and remains uninstalled. The original memory observation is live at
6.11 hours without gaps or OOM; ten-hour and sustained-load acceptance remain
open.

Latest native increment: [worker result ingestion](native-worker-result-import.md)
adds a local-only CLI boundary for importing a worker's terminal native ledger
result against a separately prepared central job binding. Identical delivery
counts once across retries; changed results are refused, and process exit is
kept separate from validation/publication. It is source-only. Central
policy/history preparation and automatic native execution remain open.
Verification passes 132 ledger, nine capability, 43 telemetry and seven CLI
ledger cases, plus all-target/all-feature Clippy, formatting, TypeScript
contracts build and server typechecking.
Matched private `59d8168` / public `fbdaf459` also passes 1,158 server and 17
mock cases plus production builds and artifact/credential guards. The existing
memory observer remains live at 5.10 hours without gaps or OOM; ten-hour and
sustained-load acceptance are still open.

Latest replay correction: [request binding](worker-execution-results.md#request-binding-follow-up)
rejects changed work, routes and execution controls on a reused dispatch request
or active-work join. Both APIs run the `5328f873` backport; independent central
and worker live checks reject converting an owned dry-run to real execution,
then return its original receipt on an exact replay. The matched private build
passes 1,158 server and 17 mock cases. Native policy/history transfer remains
open. At 15:31 UTC, the unchanged memory observer had 4.63 hours without OOM or
gaps; full ten-hour and sustained-load acceptance remain incomplete.

Latest worker correction: [execution ownership and results](worker-execution-results.md)
keeps unknown cancellations, disconnects and lost acknowledgements from
releasing work prematurely. Offered receipts survive coordinator restart, and
fresh terminal states require an observed result. Public `b27bdf55` is deployed
to both APIs and the shared web; a pinned Windows WSL dry-run returned its
observed exit code and the worker subsequently had zero active sessions.
The matched private assembly passes 1,155 server and 17 mock cases. The
unchanged memory observer has reached 4.38 hours without OOM; ten hours and
sustained agent load remain unverified. Native remote handoff still
needs central ledger/policy transfer; this is its execution boundary.

Latest staged control: [factory spend ceilings](factory-spend-ceilings.md)
adds explicit daily/monthly recorded-cost guards, UTC rollover, pause notices
and shared status. CLI and rendered scenarios pass. This is not enabled on
the live controller; the existing memory observation remains uninterrupted.

Latest usage delivery: [combined recorded usage](combined-recorded-usage.md)
is live at public `39c29be4`, with chat and factory source coverage, measured
subtotals, account/model/project/work/day grouping and JSON export. Both live
sources pass 7/30-day acceptance. Missing billed cost remains unknown. This
supersedes the staged-only native usage status below; GitLab auth, distributed
factory handoff and the long memory observation remain outstanding.

Latest coordination correction: [central claim participation](central-claim-participation.md)
lets central and registered workers arbitrate the same leases without a
self-polling registry entry. Live cross-host claim tests pass. Central's
controller now loads the existing API credential and shared claim URL under
its original memory cap. Matching Windows factory policy remains next.

Latest reviewer check: [Windows review-provider readiness](review-worker-readiness.md)
provisions central's existing configured provider and completes a real worker
chat with the exact routine-review model. No new login or factory policy change
was needed. The worker's missing factory routes and incomplete cross-controller
claim configuration remain the next execution gates.

Latest execution/lifecycle correction: [worker execution and chat runtime
release](chat-runtime-lifecycle.md) reuses the existing login in WSL, completes a
real worker chat and fixes archived ACP process retention. Three live worker
archive cycles confirm complete process-tree exit. The matching private build
passes; the patch is deployed to worker and central. Complete factory execution
and the broader usage/auth acceptance remain open.

Latest execution check: [capacity and worker diagnostics](execution-capacity.md)
rules out the current event-query payload as the primary API memory bottleneck,
identifies the central admission constraint and worker login gates, and fixes
the Windows worker's blocked read-only decision route. Central can now read
that decision state. Actual provider execution remains unverified.

Latest connection correction: [WebSocket welcome](websocket-welcome.md)
removes the duplicate status query and keeps absent work observations absent.
The live TLS initial connection/reconnect, browser unavailable-state checks
and matching private build pass. Shared web is `6c473dd6`; the API receives
only the tested welcome backport, retaining the earlier server fixes.

Latest API correction: [provider startup](provider-startup.md) removes the
pre-listen status query, with honest pending/failure/recovery readiness. Public
and private regressions pass; the live node answered while initialization was
pending and subsequently became healthy. The separate welcome status barrier
was subsequently removed by the correction above.

Latest deployment: [capacity notifications](capacity-notifications.md) makes
pre-launch waits quiet history, corrects matching old false failure alerts,
and preserves real alerts and their receipts. Shared web `0ce6e7fb` and the
notification-only API backport pass asset, state-preservation and live TLS
protocol checks. The resource constraint and complete pipeline remain open.

Latest live work: [central recovery and memory attribution](live-central-recovery.md)
restored the tailnet dashboard/API, preserved credentials and sessions, and
verified every deployed JavaScript chunk. An earlier 15-minute controller
trial passed, but its extended trial reached the memory guard. The streamed
recovery build `befb656a` passed a 3,605-second observation under the original
cap without restart, swap or OOM and remains running. This covered orchestration
and capacity deferrals; sustained agent load remains unverified. Native Windows Overview/Factory load with saved
owner access. Complete worker diagnostics, Git transport, UI clarity and
long-soak acceptance remain open.

[Persistent panel sessions](persistent-panel-session.md) replace the handoff
owner-token response with an ordinary revocable controller cookie. The public
release installer and combined private build pass; live rollout and real
GitLab SSO acceptance remain open.

[Provider quota identity and freshness](provider-quota-freshness.md) is deployed
as shared web `99f8cca1`: unrelated instance identities stay separate, failed or
old readings stay historical, and native probe markers stay out of subscription
accounts. Every served asset is verified. Persistent auth API rollout awaits
the actual GitLab proxy inspection; its cookie-forwarding patch passes ten
isolated nginx transport cases and has not been applied to production GitLab.

Backlog rechecked 2026-11-06: 37 open core issues, one open enterprise issue
and no open companion issues. Excluding mobile #1228/#936, parent items
#1591/#1513/#1506/#1430/#1337/#935 and parked #1403/#1235/#863 leaves 27
individual desktop/web tickets. Including the owner's two new usage/auth
reports gives 29 scoped items. Those reports remain unfiled; deployed partial
fixes are not closure evidence for the remaining live acceptance work.

[Remote dispatch execution controls](remote-dispatch-options.md) fixes a worker
WebSocket omission that could turn a requested dry-run into normal execution.
The central API and Windows WSL worker are updated; an owned pinned dry-run
completed. The current combined private build passes. Native factory handoff,
live GitLab auth acceptance and the ten-hour memory soak remain outstanding.

[Chat rollup completeness](chat-rollup-completeness.md) corrects partial token
sums and inferred cache readings. Central and the shared web are updated; the
live 30-day report retains all 85 observed turns rather than grouping only the
three measured turns. Missing measurements remain unknown. Unified chat/factory
reporting under #1538 remains open.

[Canonical execution usage](canonical-execution-usage.md) introduced native
project/work-item grouping and a streaming dispatch-usage source for #1538.
Per-attempt and legacy summary readings are counted without adding mirrored
totals twice. The [combined report](combined-recorded-usage.md) now deploys this
source alongside chat, its HTTP endpoint and Telemetry view.

[Chat token categories](chat-token-breakdown.md) now survive ACP conversion and
durable history, and the session drawer refuses complete totals when replies
have missing measurements. Focused protocol, replay, unit and rendered component
checks and the final private build pass. Central's adapter and shared web are
deployed with verified compiled preimages and served asset hashes. Central is
healthy; the Windows worker is updated. Fresh Codex and exact Mistral route
samples reconcile the apparent token gap through reported cache-read tokens.
Historical/billing reconciliation and provider quota availability remain open.

[Native reviewer readiness](native-review-readiness.md) provisions matching
Windows policy and packaged role agents, verifies real native reviews through
the production gate, and fixes routine/escalation overlap that could bypass a
review-cycle cap. The verified CLI is installed on Windows/WSL and central;
84 review tests, Clippy and release build pass. Subsequent review-budget work
records the current controller/observer identities. Real distributed factory
progression and GitLab persistent-auth deployment remain open. The fresh #1525
gate is now review-budget exhaustion; five current-generation records marked
`no_attempt_started` are being counted and require a writer-lifecycle audit.

That [review preflight audit and correction](review-preflight-budget.md) now
establishes zero attempts and no contradictory execution evidence for all five
records. Shared dispatch/status accounting removes the false hold without
resetting history or changing caps. Native and private checks pass; the CLI is
deployed on Windows and central, and the installed live same-head status is
unheld. The factory now reaches context preparation and its real node-capacity
gate. A sampler timeout was diagnosed separately; a corrected, PID-bound
ten-hour observation is verified running against the current controller.

[Factory admission evidence](factory-hang-recheck.md) now identifies the live
node-memory wait on #1525. Shared web `c0668cae` exposes the exact recorded
refusal in Waiting and stops clipping it on Overview. Production capacity
guards are unchanged; real remote execution and complete pipeline acceptance
remain open.

Windows follow-up: [worker recovery](windows-worker-recovery.md) updates the
old worker, corrects its central origin, resumes its logon keepalive and verifies
reachability from central. The tested launcher retry fix is installed in the protected
Program Files script, with rollback; its task and worker remain running.

Latest Mac work: [native client update and temporary central test](mac-client-update.md)
installed and launched 0.3.0, fixed local app-bundle sealing, passed native and
central HTTP/WebSocket/asset/cost checks, and removed the owned build tree.
The live central now serves verified assets; full live acceptance remains open.

Latest activity repair: [account execution evidence](account-activity-evidence.md)
uses exact session/worker account identity, node ownership and worker freshness.
Claims, provider transport IDs and the first sibling account no longer imply
working. The matched `6b101b98` public package / `8ec34f7` private assembly pass
builds, actual isolated installation and assembled Windows UI workflows. Live
acceptance remains.

Latest memory trial: [48 passes in one controller](long-controller-memory.md)
completed 192 synthetic starts and graceful shutdown. RSS rose overall with
late drops; this does not resolve allocation attribution or prove a plateau.
Partial event-size observations are retained separately from paused RSS samples.
A later 12-pass allocator diagnostic separates allocator free space from a small
in-use proxy; one optimized single-pass check also passed. Neither establishes
production leak attribution or long-running stability.

Latest quota recovery: [failed reads and pending retries](quota-refresh-recovery.md)
retain recorded costs/allowances while page and navbar make current usage and
eligibility unconfirmed. Successful reads restore the fresh view. Windows unit,
full browser recovery and typechecking pass, with inspected desktop/mobile
layouts. The matched `cd67c949` package and private `9d67290` assembly include
both usage repairs; assembled UI, native isolation and local Auth/relay pass.

Latest usage repair: [recorded account costs](account-cost-attribution.md) now
come from canonical ledger execution identity, rather than summing overlapping
candidate totals. Named accounts, unattributed history, estimates, unknowns and
measured zero remain distinct. Rust, real-CLI, Windows unit/component/browser
and worker-relay checks pass. The current package/private assembly include it
and the WSL user CLI is updated with rollback. Real provider acceptance remains.

Latest update recovery: the [server update banner](server-update-recovery.md)
resumes observation after reopening, stops polling on unmount, preserves
uncertainty through interrupted reads and identifies the server component.
Six Windows browser checks, typechecking and the production build pass.
The matched `fdc9567f` package and private `dec85e7` assembly include it; all
six cases also pass against the exact private assembly. Live deployment remains
open.

Latest memory repair: [streaming ledger reads](streamed-ledger-memory.md) removes
an extra whole-file text allocation while preserving complete decoded history.
119 ledger and 198 controller tests, formatting and Clippy pass. The twelve-pass
comparison observed lower peak/final RSS, but growth and live attribution remain
unresolved. The optimized WSL user CLI is updated with rollback; Windows account
metadata and Mistral preflight reads pass.

Latest worker observability: [worker factory state](worker-factory-observations.md)
now reads a registered worker's controller, last decision and gates for a chosen
declared profile, without shell access or remote mutations. Six HTTP checks,
eleven component checks and the full sample Nodes browser flow pass. Private
`9d67290` / public `cd67c949` passes 1,106 server and 16 mock cases, build guards
and current browser transport checks. Fresh native isolation and real local
Auth/relay evidence also pass. Live acceptance remains open.

Latest delivery progress: the [current public recovery package](current-public-release.md)
at `cd67c949` includes pipeline, memory, updater and account-cost/quota repairs.
It passes actual isolated release installation with Windows npm first on PATH
and byte-for-byte HTTP reads of all 29 JavaScript chunks. The installed WSL CLI
matches the candidate; native desktop/settings and worker binaries retain their
hashes. The desktop loads the central dashboard, so live deployment is required.

Latest recovery progress: [failed factory refreshes](kanban-refresh-recovery.md)
retain recorded jobs while marking execution/capacity unconfirmed, disable
assignment and keep the warning through retries until success. Windows unit,
component and full-browser checks pass; desktop/mobile captures were inspected.
Stale-only worker rosters also show unconfirmed activity; mixed rosters count
only confirmed running work and retain stale observations. Private `fd4d761` /
public `7db57658` passes the combined build and 35 assembled board/store checks.
All 15 Windows Kanban component checks pass. Installed apps and live services
remain unchanged.

Latest pipeline UI progress: [claim and execution evidence](kanban-claim-evidence.md)
keeps claimed issues visible while separating ownership from confirmed running
work. Stale workers remain uncertain, existing gates remain visible, and claimed
work cannot be assigned twice. Seven unit and thirteen Windows component checks,
typechecking and the web build pass; desktop/mobile captures were inspected.
Private `233947f` / public `9f39774e` now includes the UI change in the assembled
app: 1,099 server, 15 mock and seven assembled Kanban checks pass, with
typechecks/builds and guards. Installed artifacts and live services are unchanged.

Latest compute progress: [bounded delivery and enrollment repair](compute-delivery-authorization.md)
keeps slow or denied result replay within a shared network budget and allows
same-origin credential repair without rerunning completed work. Private `3bfc79b`
with public `819937bf` passed 1,099 server, 15 mock and 100 dedicated WSL cases
(one platform skip), plus typechecks/builds and real local Auth/PostgREST/relay
checks. Per-receipt UI and hosted authorization acceptance remain open.

Latest worker progress: [ledger ownership](worker-ledger-retention.md) releases
full history before serial, parallel and TUI execution, retaining bounded retry
context. Controlled real-CLI probes confirm lower retained memory; controller,
prior-context and scheduler checks pass. The optimized `819937bf` recovery package
is verified and installed as the WSL user CLI with rollback. Live memory
attribution and a long soak remain required.

Latest observability progress: [native Windows memory diagnostics](windows-memory-diagnostics.md)
now collect process working sets and mark unavailable CPU/load/collection data
explicitly. Six native tests and typechecking pass. The [live recheck](live-central-8443.md)
still finds failed dashboard asset GETs and protected API access; the private
token card timed out and Windows SSH is denied. No live service was changed.

Latest native progress: [Windows updater configuration](windows-updater-config.md)
fixes key discovery when HOME is unset, preserving XDG/file/environment overrides.
All 36 ordinary Windows desktop tests and the NSIS build pass. The closed local
desktop test installation is updated with rollback and unchanged settings.
Publisher key/feed provisioning and signed update acceptance remain open.

Latest installed-runtime progress: the [current public candidate](current-public-release.md)
at `77209892` packages optimized memory/scheduler repairs and passes isolated
release installation, public server/mock checks and cold asset reads. The WSL
user CLI is updated with rollback; both Windows desktop integration reads pass.
The native desktop and saved settings are unchanged. No live service was deployed.

Latest scheduler progress: [bounded controller passes](controller-pass-turnover.md)
prevent an always-busy parallel queue from retaining completed pass state
indefinitely. The real-CLI regression fails on the previous binary and verifies
that remaining jobs run in the next pass. Controller/refill checks passed;
the diagnostic follow-up also passed controller and shutdown checks. Live OOM
attribution and a long soak remain required.

Latest history progress: [streaming event reads](event-reader-memory.md) remove
the extra whole-journal text allocation and bound limited event-query retention.
The memory-limit regression fails on the previous CLI and passes on the new one.
Private `3679931` / public `a6a38c4d` passed the combined production build;
12 event, 197 controller and two CLI checks passed. Live OOM attribution remains open.

Latest factory progress: [bounded Rust output capture](rust-output-memory.md)
closes a reproduced growing-buffer/progress-queue path and rejects incomplete
reviewer evidence. Native runner, review-workflow, CLI and refill checks passed;
the `e7d088c` / `2f8c9cf5` combined build passed 1,093 server and 15 mock cases.
The new measured Linux OOM report #1681 still needs live allocation attribution
and a long soak. No deployed worker or service was changed.

Recent compute progress: [retired-claim delivery reconciliation](compute-delivery-retirement.md)
fixes a reproduced endless retry without overwriting either the local result or
cloud outcome. Explicit retired/unconfirmed receipts can be reviewed for cleanup
with fresh proof. Private `9b4ae6d` has production sources identical to the
`c013752` / public `b0cd55e1` build: 1,093 server, 15 mock and 96 dedicated WSL
passes (one platform skip), 19 browser cases and the full sample Settings flow.
Real Auth/PostgREST also passed unchanged local/cloud outcomes, retired-claim
execution denial, reviewed cleanup and retained queue/recovery/relay checks.

The preceding [workspace admission visibility](compute-admission-visibility.md)
shows pause reasons and recovery guidance separately from worker liveness, with
independent expiry and hardware-member visibility. Private `4bca434` / public
`f11b53c3` passes 1,087 server, 15 mock and 90 dedicated WSL cases (one platform
skip), plus typechecks/builds and five new cloud Settings browser cases.
Real SQL/Auth/PostgREST also passed admission reporting/expiry and retained
queue, recovery, maintenance, authorization and relay checks.
Hosted reporting, uncertain/legacy state and orphan metadata remain unfinished.

The preceding [receipt retention controls](journal-retention-controls.md)
expose the separate journal admission limit and owner-confirmed cleanup in shared
Settings. Unverified receipts remain held; interrupted removal is resumable.
Private `5ef4458` / public `189f01e3` passes the combined production build.
The unchanged production sources passed 89 dedicated WSL cases, 18 browser
cases and a full Settings workflow. Hosted admission visibility, uncertain/legacy
state and orphan capability metadata remain unfinished. Final-pair real
Auth/PostgREST also passed maintenance and retired-claim execution denial,
queue/crash recovery and retained authorization/relay checks.

Latest Windows evidence: [native output acceptance](windows-native-output.md)
verifies a 20 MiB streaming save, corrupt-block rejection and cancellation with
real hidden WebView2 filesystem objects. The existing shared implementation
passes without a new native bridge. The committed `d531a72f` / `f322996b` pair
also passes the combined production build. Interactive picker and installed-release
acceptance remain unfinished.

The [saved-output review controls](saved-output-controls.md)
connect owner-authenticated downloads and explicit retention actions to shared
Settings. Private `360ff0c6` / public `252d4b05` passes 1,072 server cases,
14 sample cases, 11 browser behavior cases, a full Settings workflow and 77
dedicated WSL cases, plus disposable real Auth/PostgREST checks. Installed Tauri
large-file saving, hosted use and legacy/capability-metadata maintenance remain
unfinished or unverified.

Earlier enterprise evidence: [verified crash reconciliation](enterprise-compute-reconciliation.md)
retains automatic queued compute and bounded archives. Private `ac04a1a4` /
public `2cc5e3fd` passes the combined build, 68 dedicated WSL checks (one platform
skip), real Auth/PostgREST consumer kill/restart and normal queued workflows.
The launch handshake, systemd slice path and empty-scope retirement repairs
passed seven startup offsets and twelve repeated formerly failing timings.
Legacy/unknown state and metadata maintenance,
production runtime/client and forge access, aliases, ledger integration,
full shared-hardware acceptance and migration remain unfinished.

The preceding [durable inference retry receipts](enterprise-inference-receipts.md)
fixes a reproduced duplicate-request path and records the clean private `14c16068`
build against public core `16e9ecc6`. Real Auth/PostgREST concurrent retry and
relay checks passed. That verification preceded the queued-compute and isolation
work linked above; aliases and migration remain unfinished.

Latest factory evidence: [exact account routing](factory-account-routing.md)
adds node/account/model selection, preserves account identity through CLI retries
and lease recovery, and replaces optimistic Sent feedback with acknowledged or
explicitly uncertain submission state. This remains local and undeployed.

Latest desktop evidence: [unfinished navigation recovery](desktop-navigation-timeout.md)
adds bounded recovery and keeps connection controls usable during stalled status
reads. MacBook work remains deferred.

Windows [provider credential transport](windows-provider-transport.md) now passes
a real WSL synthetic two-account lifecycle and bounded-process checks. Real
provider and per-account agent execution acceptance remain pending.

The [account execution pass](windows-account-execution.md) now verifies concurrent
synthetic launches, credential revocation and correct safe attribution after a
rebind. Matching `b738b9d2` desktop/CLI artifacts were installed locally. Real
provider execution, metered usage and live factory acceptance remain pending.

The [Windows Mistral sign-in path](windows-mistral-sign-in.md) now implements
native browser-to-WSL verification. Native cookie-store isolation, staged
failure/success handling and actual installed-node preflight passed. Actual
provider sign-in and live readings still require owner acceptance.

Branch: `beta/factory-observability`. This is development evidence, not deployed beta acceptance. See [Beta acceptance](../../BETA_READINESS.md) for the remaining workflows.

## Verified changes

- A stalled WebSocket handshake releases Connecting and retries.
- Work details show all matching blockers, review evidence, and recent controller decisions, including refused routes.
- Kanban explains receipt-based review pauses instead of promising a reviewer that the controller will not launch.
- Sample control planes identify themselves; work, usage, and update notices are explicitly labeled simulated.
- Account configuration, work history, and controller decisions stay scoped to their selected project across delayed responses and rapid switches.
- An unavailable auth observation stays unknown without hiding healthy sibling accounts.
- Preview auto-detection requires an HTTP response from the actual upstream address, rather than relying on a TCP handshake.
- Readiness, usage trends, and controller status reject late results from another project. Finishing a controller action after switching projects cannot restore the previous project's status.
- Login repair keeps checking after repeated temporary read failures, clears the error on recovery, and stops polling on completion or unmount.
- The Factory screen puts review and blocked work before manual dispatch, displays blocker messages and paused-review explanations inline, and searches by issue, title, or explanation. Each row has an explicit Details action.
- The development launcher starts the npm CLI through Node on Windows, forwards single-surface arguments and exit status, and keeps the sample server's custom port aligned with both web proxies. It stops only its own Windows process trees when the paired command ends.
- Profile can start browser/device sign-in for isolated Codex and Claude accounts on the connected central node. This owner-only operation uses the existing private repair broker and Rust's instance login command. It verifies that exact account before reporting success. Closing the panel cancels an active sign-in, including a start whose response arrives late. Saved credentials and other runners show applicable connection help instead of an unsupported generic login command.
- Profile editing submits only explicitly changed fields and prefills saved artifact roots and target branches from current nodes. Older nodes leave unavailable settings untouched. Failed creates/edits retain entered data; failed deletes remain open with their error. New profiles use the entered ID. A failed post-save list refresh releases loading and offers retry; an earlier read cannot restore the pre-save list.
- Profile now edits factory development and production environment sources through explicit keep/replace/remove actions. Existing source values and file contents stay private. Failed writes retain input and show a fixed error rather than echoing CLI source text. A forced post-save configuration read supersedes an older pending read.
- Profile's Factory checks editor loads command text only on an explicit owner request, edits individual complete commands, patches only changed lists, and reloads to verify the result. Empty arrays explicitly clear a list. Failed or different readbacks preserve the operator's edits. Command text stays out of public profile and configuration views.
- CLI profile add/set accepts repeatable singular `--validation-command` and `--auto-fix-command` flags, preserving commas, quotes, line breaks, and leading hyphens. Legacy comma-delimited plural flags still work. The profile grammar was moved unchanged into its own module in a separate commit to respect the source-size limit.
- Interactive chat applies development application variables through the existing account-specific launch paths. Rust owns the shared parser and filtering. Default accounts use a bounded private CLI pipe; named accounts resolve values inside their native launcher before applying account state and credentials. Production, provider/account configuration, control-plane values, and work-scoped credentials are excluded. Relative chat paths resolve from the configured checkout. Failed sources stop startup with fixed errors, and cancellation/disposal prevents a delayed environment read from launching a retired ACP process.
- A failed project read now shows unavailable instead of waiting indefinitely for a server welcome. Its menu offers forced retry and connection settings; a saved project name survives an outage. Project reads refresh on socket reconnect. Two focused Windows component tests passed, the five navigation regressions passed, and web typechecking passed. Desktop and 390px recovery-menu screenshots were inspected.
- Central claim admission now records structured acquired/refused evidence in the existing events log before backend execution. CLI and HTTP status expose the latest profile/repository/central-scoped attempt, and Factory shows its work ID and time. Missing, unreadable, and unknown evidence remain distinct from admission. The server preserves its configured role and drops observations from a different central. No claim policy, fallback, controller action, or lease behavior changed.
- Fleet polling carries workers' claim observations per profile into the central dashboard. Factory identifies the source machine and central origin, filters other profiles, and marks cached readings when a poll fails or returns another profile. Recovery replaces old observations. New worker fields pass through a bounded projection that rejects credential-bearing origins and malformed timestamps and treats unknown outcomes as unknown.

## Checks

- A current Windows x64 app and NSIS/MSI candidate at `bbc37aa5` were built and
  packaged with a matching prebuilt WSL worker. The actual per-user Windows
  desktop installation was upgraded and launched; settings were preserved and
  installed payload bytes were verified, allowing only Tauri's documented NSIS
  bundle marker. See [Windows candidate and upgrade](windows-release-candidate.md).

- The final shared-web sweep at `0ee85993` passed all 160 Windows workflow tests,
  195 Windows component tests (one platform-specific skip), 130 unit tests and
  web typechecking. The skipped POSIX installer-command case passed separately
  with Linux Node under WSL. See [workflow sweep](shared-web-workflows.md) for
  coverage and acceptance limits; it is not live factory or provider evidence.

- Memory retention (#1680): an isolated Node reproduction failed before the fix because the server retained the complete streamed transcript. The unused duplicate cache/accessor was removed, diagnostic strings now retain a bounded tail with an omission notice, and queued cancellation uses existing session state instead of permanent ID tombstones. All-package typecheck, server production build, 995 server tests (one opt-in skip), and twelve mock tests passed. See [memory evidence](memory-retention-1680.md); the user's Mac process and deployed source remain unverified.

- Central admission pass: formatting and all-target/all-feature Clippy with warnings denied passed. All 2,481 unit tests and all 51 integration targets plus doc tests passed across the initial run and a corrected-PATH resume. The initial run stopped at an installer fixture because its Rust wrapper omitted npm from PATH; restoring the existing npm shim made that target and every remaining target pass. An automated comparison against Cargo's target list verified terminal passing results for every integration target. Logs: `/tmp/parasaur-beta-claim-rust.log` and `/tmp/parasaur-beta-claim-rust-resume.log`.
- The same pass completed all-package typechecking, server production build, 991 server tests (one opt-in skip), twelve mock tests, and the web production build. Six focused node/fleet boundary tests passed, including HTTP CLI forwarding, multi-profile partial outage/recovery, wrong-profile provenance, malformed fields, and secret-bearing origins. The Windows Factory browser flow passed local/remote refusal, later admission, unknown/missing history, stale source warnings, and profile isolation; desktop and 390px screenshots were inspected. Server log: `/tmp/parasaur-beta-claim-server.log`. Initial browser attempts hit occupied ports; the successful run used free isolated ports and stopped its owned servers.

- Workspace/chat environment pass: full Linux server suite 988 passed, one opt-in skip, zero failures. The first full attempts identified obsolete CLI fixtures; the shared empty-environment fixture and the standalone OpenCode/wake test environments were updated explicitly. Focused ACP/headless/environment/instance tests passed (67), including the final shared-startup ordering and abort propagation. Root typecheck and the server production build passed. Rust environment tests (two), all credential-binding CLI tests (22), contracts (seven), source structure (32), formatting, and all-target/all-feature Clippy with warnings denied passed. The required full `cargo test --locked --all-features` rerun at `11eb409d587164137a9f65c2dec675a600d6040a` then passed: 2,479 unit tests and every integration/doc-test target; the wrapper recorded exit code zero.
- Real CLI-to-Node-to-child smoke: an isolated development file reached a headless agent through the production private environment reader. The selected parent account/state stayed intact, production and control-plane values were absent, and the next invocation observed a file edit. The actual named-Codex CLI regression independently preserved its selected saved credential and state root. No provider login or real job ran.
- A subsequent ACP startup-recovery regression passed with the complete 12-test ACP integration file: a failed environment read releases the pending startup and the next request can connect successfully. Private CLI reads also receive the turn's abort signal so cancellation stops the pending lookup.
- Environment editor verification: two component tests and its stateful sample browser save flow passed after the copy update. Desktop (1100px) and narrow (390px) screenshots were inspected; labels and source selectors wrap without horizontal overflow. The web production build passed.
- Current-source native Windows desktop: typecheck, Vite connection-screen build, and all 26 Rust desktop tests passed. With the now-installed MSVC toolchain, Tauri produced the release executable and `apps/desktop/target/release/bundle/nsis/GAH_0.3.0_x64-setup.exe`. The final bundle includes `4b50689b`, which puts Connection and Owner access before provider/setup sections. Installer SHA-256: `dedb91a67546a7502a82358dfee5489bcd091cbd3867721289a20f1139ae0f71`. The executable was launched with an isolated child-process USERPROFILE and loaded Overview, Factory, and bundled settings against the existing port-38000 sample UI (web revision `421407b6`). The final connection-first settings screen was inspected through native Computer Use. Typecheck and packaging passed again after the HTML-only refinement; Rust desktop source was unchanged. The NSIS installer was built but not installed or published. Release compilation reported three existing warnings; the build succeeded. The layout detector reported no findings in its degraded regex mode; its missing parser modules mean it cannot establish computed contrast or selector/layout correctness.
- Windows installer decision/contract checks passed with PowerShell's execution policy scoped to that test process. Downloads, service operations, elevation, restart, and user initialization were mocked; this did not run the installer itself.
- Full Windows browser end-to-end suite at `421407b6`: 159 passed (3.5 minutes).
- Full Windows component suite after isolating the shell fixture: 193 passed, one POSIX execution test skipped. The skipped test passed separately with Linux Node under WSL. The first broad component attempt bypassed its fake curl through Windows bash/WSL forwarding and stalled in real setup; its two identified setup process trees were stopped. UI generation/copy assertions now run independently on Windows, and the shell helper refuses Windows before spawning, has a timeout, and removes its fixture.
- Root `npm run typecheck`: passed across contracts, shared, server, mock, and web.
- Full core `cargo test --locked --all-features` at `421407b6`: passed under WSL after canonical Git checkout normalization. All 2,477 unit tests and every integration/doc-test target completed, including controller refill (19), CLI integration (261), contracts, and source-structure checks. The later Windows fixture and desktop HTML changes do not change the tested core Rust source.
- Web production build: passed.
- Full Windows frontend unit suite: 130 passed after the environment-source refresh regression.
- Focused Windows component suites: connection recovery, work drawer, blocker actions, profile configuration, and account creation passed.
- Windows browser flows: factory environment/history, profile settings and project switching, and live account status passed, including narrow-screen checks.
- Latest Factory browser run: six passed, including searching a paused-review explanation and opening its details. Desktop and 390px screenshots were inspected. Login repair and repository CLI installation component suites: five passed.
- Development launcher regressions: four passed on native Windows and four under WSL. A real Windows sample launch on port 38776 returned sample identity both directly and through Vite on port 3000; its owned process tree was stopped afterward. The first smoke attempt queried before the sample server announced readiness; the corrected check waited for both servers before making requests.
- Full Linux server integration run under Ubuntu/WSL after adding workspace checks: 983 passed, one opt-in test skipped, zero failures. The subsequent CLI-response shape guard passed the focused CLI/owner HTTP suites (32 tests).
- Mock control plane suite: twelve passed. Named-account sign-in returns an explicit sample-only instruction and never starts a real login. Environment sources retain only per-profile presence flags in memory, round-trip edits, and clear on reset without storing source values. Checks round-trip in memory and stay out of public profile metadata. Route-coverage tests reset their fixture before each independent call so an earlier DELETE cannot turn a valid resource read into a false missing-route report.
- Named-account and login repair component suites: ten passed; seven browser flows covering auth health and profile settings passed. Sign-in screenshots were inspected at 960px and 390px. Server and web production builds passed.
- Actual Rust CLI isolation regression: two Codex and two Claude accounts wrote to separate state directories using fake provider executables; the owner's default login files remained unchanged. Rust formatting passed. This checks routing and isolation, not a completed sign-in to the real provider services.
- Profile editor browser regressions: five passed, including input retention, explicit IDs, and branch-only patches. The five existing profile-settings browser tests also passed. Profile editor screenshots were inspected at 1440px and 390px; catalog projection tests passed (10 tests).
- Workspace metadata: the Rust projection test passed; a temporary real-CLI HTTP server returned the saved artifact path and `release/stable` branch, accepted a name-only PATCH, and returned those unchanged settings afterward. This used an isolated config and no provider login or live node.
- Rust contract checks: seven passed. Source-structure checks: 32 passed after giving the Linux test copy independent Git metadata and canonical LF executable/shell files. The initial snapshot lacked `.git` and retained CRLF shell text, so its eight structure failures were not valid repository results. Original Windows files and Git metadata were not changed by that correction.
- Environment source editor: four focused component tests passed, including the existing source-redaction checks; screenshots were inspected at 960px and 390px. The integrated Profile save refreshed configured status without rendering a private source canary. CLI argument tests passed (30 tests), and the web production build passed.
- The integrated environment editor also passed against the real stateful sample routes, with no stubbed configuration response. The local sample backend was refreshed to load this behavior for the running desktop test UI.
- A real Rust CLI/HTTP environment-source smoke test replaced the development path and removed the production reference, confirmed both changes through the CLI, preserved all three fixture files and their contents, and confirmed settings responses contained neither source paths nor the fixture canary. It used isolated local files and no provider or live node.
- Factory checks: four component tests passed; the checks/environment/profile browser group passed all 12 tests. Editor screenshots were inspected at 1100px and 390px. The actual Rust CLI add/set/show regression preserved complex commands, accepted the legacy flags, and cleared both lists. Formatting, all-target/all-feature Clippy with warnings denied, seven contract tests, and 32 source-structure tests passed.
- A real CLI/HTTP checks smoke test saved and reloaded comma-containing, quoted, multiline commands, cleared just validation, preserved auto-fix configuration, kept command text out of public settings/profile responses, and confirmed saving did not create the marker that execution would have created. This used an isolated config and executed no configured check or provider.
- The opt-in paid-route HTTP test was exercised separately against the actual Rust CLI and an isolated ledger: five Rust/support tests passed.
- Execution identity, AGY pool identity, and usage telemetry regression targets: 28 passed under WSL.
- Native Windows preview-proxy tests: 14 passed.
- OpenAPI YAML parsing and `git diff --check`: passed.

## Platform notes

The full server fixture suite requires POSIX executables and now refuses native Windows before running. Validate it inside a Linux/WSL checkout with separate Linux dependencies. The WSL test snapshot used canonical LF fixture scripts; `.gitattributes` now preserves executable-compatible line endings for future checkouts.

Structure checks also require a real Git index and baseline history. The Linux snapshot now has its own shallow metadata copied from this branch and the existing `origin/main` ref; all tracked sources were restored through its independent Git index to canonical checkout line endings before the full Rust rerun. Normalizing executable fixtures alone left CRLF documentation and service templates in the initial copy and caused four invalid unit-test failures. It does not share writable Git metadata with the Windows workspace.

The earlier Windows rendering checks used the local sample backend. The current
installed native executable is `c1a55cf0` and is closed; its unchanged source did
not require another native install. The WSL user CLI and matched CLI/server/web
recovery package are now `16e9ecc6`; see [current release evidence](current-public-release.md).
The existing WSL worker continues using its separate release. These local
checks do not establish that the deployed factory is running the new code.

Named-account browser sign-in currently operates on the connected central node's configured accounts. Fleet login-health rows still retain their existing manual named-instance guidance. Real provider account consent, saved-credential replacement, and remote-worker named login remain acceptance work. The local sample backend was deliberately restarted to load its new explicit sign-in response and was checked through the same port-38000 Vite proxy used by the native test shell.

Development sources now also supply application variables to new chat agent processes. Production still requires `gah dispatch --prod` and is never a chat fallback. Chat protects provider/account settings, state paths, control-plane variables, and work-scoped credentials; it does not replace factory dispatch's existing environment policy. Long-running chat processes keep their initial values, while one-shot invocations reread the file. Check editing uses lossless command transport and an owner-only view. Other execution nodes retain their own configuration. Remote-node and real-provider workspace acceptance remains pending.

The saved desktop central origin answered health checks but returned 404 for the dashboard and `/api/info`. The actual Parasaur address and Michael's runtime state remain unavailable; no live claim, review gate, retry budget, or deployed configuration was changed.

The owner confirmed the saved tailnet hostname is intended to host both Hermes and Parasaur. A fresh request identified its current HTTPS service as Python aiohttp, `platform: hermes-agent`, version 0.21.4; the host was online in Tailscale. Direct ports 3773 and 3774 did not accept these Windows probes. SSH as the owner's specified `khing` user reached the host but rejected this Windows key. MacBook relay tools were unavailable in this session. A read-only routing/service check from the MacBook is pending before assigning Parasaur a separate HTTPS port; Hermes routing has not been changed.


## Enterprise compatibility validation

The local private submodule branch now pins the validated workspace core `11eb409d` and preserves its factory, account, navigation, and reconnect changes. Relay fixes prevent closed sockets from reopening after delayed replies and retain node-local authentication for named accounts. The combined production build passed 991 server tests (one opt-in skip), twelve mock tests, and artifact/input guards. Separate browser transport regressions passed. Disposable PostgreSQL migration replay and real Auth/JWT, edge, PostgREST, HTTP/WebSocket relay, recipient isolation, and removal checks passed. Private source and detailed evidence remain in the private submodule; nothing has been pushed or deployed.

Windows Chromium also passed the private UI's two-account invitation workflow against the real disposable account services: owner sign-in and invite, recipient sign-in and accept, member controls, then owner removal and denied recipient refresh. Desktop and 390px screenshots were inspected. A repeatable check is committed in the private submodule, with loopback and disposable-account restrictions. Test browser contexts and all owned fixture services were stopped, and the temporary Windows session file was removed. This validates the shared UI locally; live hosted access and real node/provider work remain pending.

MacBook SSH was also tried from WSL and rejected the available key, matching Windows. The private password request expired without a credential; the authorized MacBook login and its existing SSH route to the host as `khing` remain unavailable. No live service or Tailscale routing has been changed. Hermes can retain its current HTTPS listener while Parasaur uses a separate listener after the actual host configuration is inspected.


## Live service discovery update

The local memory retention fix and validation are recorded in
[memory retention #1680](memory-retention-1680.md). The subsequent release
packaging correction and cold-install/source-build checks are recorded in
[release runtime validation](release-runtime.md). Neither has been deployed.
An exact-revision Linux candidate also passed its actual optimized CLI release
installation against a disposable checkout; see
[Linux release candidate](linux-release-candidate.md). MacBook acceptance is
explicitly deferred by the owner.
The Windows finishing pass also added
[native dashboard load recovery](desktop-dashboard-recovery.md), verified in
an isolated native webview against failed app assets. Native link activation
remains an explicit acceptance item because Windows input geometry was absent.

Parasaur is confirmed at `https://hermesagent.tail82695.ts.net:8443`, with central role and version 0.2.0. The real Windows desktop connection and the owned native test window were corrected to that port. The earlier statement that the Parasaur address was unavailable is superseded. Its HTML returns 200, but both referenced JavaScript and CSS GETs return 500; asset HEAD succeeds. The native window reached the correct origin but could not render the application. Protected API reads require a central credential, requested through a private card. See [live central evidence](live-central-8443.md). No host routing or service was changed.

The combined private production build for core `2e78ce20` and private source `443a5d4` passed: 994 server tests, one opt-in skip, twelve mock tests, server/web production builds and typecheck, and private artifact/input guards. Its manifest binds those exact revisions. The private core pin includes both project connection recovery and local/fleet claim admission evidence. This remains a local build; the live v0.2.0 host does not establish deployment of these changes.
