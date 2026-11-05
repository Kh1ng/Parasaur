# Beta finishing pass: local validation

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
