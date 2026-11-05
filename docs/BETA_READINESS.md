# Beta acceptance

This is the acceptance checklist for the owner's finishing pass, begun 2026-11-02. GitHub issues and pull requests remain the work tracker. A checked implementation or passing fixture test does not substitute for live deployment evidence.

## Factory

- An eligible issue proceeds through claim, implementation, validation, review, repair where required, and publication under the configured policy.
- One blocked item does not prevent unrelated eligible work from advancing.
- A crashed or stalled worker releases capacity through the existing recovery and lease rules.
- Review receipts, stale evidence, retry ceilings, capacity deferrals, dependencies, and human gates have visible explanations and next steps.
- A card says whether an agent is running, a step is queued, a known condition is awaited, or operator action is required. Assignment alone does not imply execution.
- A controller pause and its last decision are inspectable without shell access to the execution machine.

## Providers and accounts

- Add and authenticate two accounts of the same provider without credential or state-directory crossover.
- Select an exact account and model for chat and factory routing; retain that identity in history and usage.
- Configure provider instances and supported models from the intended operator surface.
- Explain unavailable routes and their reset or repair conditions.

## Usage and limits

- Account, model, quota pool, subscription usage, measured API spend, and estimated cost remain distinct.
- Missing measurements display as unknown, with provenance and observation age where available.
- Configured limits are enforced at admission and their consequences appear in the UI.
- Chat and factory totals reconcile without double-counting attempts or helper calls.

## Desktop, workspace, Git, and updates

- Windows launches the Tauri application, reconnects, and recovers from an unavailable central node.
- Projects retain their execution node, workspace, environment, provider account, and configuration across sessions.
- Dirty work is preserved through worktree, commit, review, and PR/MR actions.
- Update notices identify the actual component and release; install/restart results are observable.
- Sample mode is explicitly labeled on every dashboard page, including update notices.
- Installation uses complete release artifacts where available; build tooling is not a normal end-user prerequisite.

## Enterprise and iPhone

- Provisioned users can accept a workspace invitation, connect a remote node, and execute a job under the intended authorization boundaries.
- Hosted and self-hosted configuration is documented and verified against its actual service.
- A locked iPhone receives a notification and an in-flight Live Activity, updates its state, ends it, and opens the correct conversation when tapped.
- Device revocation removes access and push registration.

## Evidence still required

- [Server update recovery](validation/beta-progress-2026-11-03/server-update-recovery.md)
  resumes persisted running updates, cancels polling on unmount and labels
  interrupted status reads as unavailable. Six Windows browser checks,
  typechecking and production build pass. This later source change still
  needs a matched package/private assembly and live acceptance.

- [Streaming ledger reads](validation/beta-progress-2026-11-03/streamed-ledger-memory.md)
  removes the extra whole-file text copy without truncating decoded history.
  119 ledger and 198 controller tests, formatting and Clippy pass. A controlled
  twelve-pass comparison observed lower peak/final RSS; growth, live attribution
  and a long soak remain unresolved. The optimized WSL user CLI is installed
  with rollback and both Windows integration reads pass.

- [Repeated controller memory](validation/beta-progress-2026-11-03/repeated-controller-memory.md)
  now has same-process measurements across twelve passes with four synthetic
  workers. Both default and process-local allocator diagnostic runs completed
  normally but showed RSS growth. Formatting/build/Clippy passed. Live heap and
  event-history attribution, repeated trials and a long soak remain required;
  no production memory workaround was applied.

- [Worker factory state](validation/beta-progress-2026-11-03/worker-factory-observations.md)
  now exposes a registered worker's profile-scoped controller status, last
  decision and reported gates through a read-only Nodes check. Six HTTP checks,
  eleven Windows component checks and the sample Nodes browser flow pass.
  Private `a10a5f2` / public `5e261e0e` passes the combined build, 100 native
  isolation checks and fresh generic Auth/relay regressions. Real worker access
  and hosted/installed acceptance remain required; the current public recovery ZIP includes this feature.

- [Factory refresh recovery](validation/beta-progress-2026-11-03/kanban-refresh-recovery.md)
  now labels retained board/navbar observations as unavailable, blocks assignment
  and preserves uncertainty through retries until success. Windows unit,
  component and browser recovery checks pass. Stale-only worker rosters show
  unconfirmed activity; mixed rosters count only confirmed running work while
  retaining stale details. Private `fd4d761` / public `7db57658` passes the combined
  build and 35 assembled board/store checks, plus 15 Windows component checks.
  Installed-app and live node recovery remain unverified.

- [Kanban claim evidence](validation/beta-progress-2026-11-03/kanban-claim-evidence.md)
  now separates claim ownership from dispatch-runtime execution, keeps claimed
  work outside intake visible, and explains the claiming machine and recovery
  condition. Stale worker observations remain uncertain and duplicate assignment
  is disabled. Seven unit and thirteen component checks, typechecking and the
  build pass; desktop/mobile layouts were inspected. The private `233947f` /
  public `9f39774e` assembled app also passes the combined build and guards.
  Installed artifacts and authenticated live factory acceptance remain required.

- [Worker ledger ownership](validation/beta-progress-2026-11-03/worker-ledger-retention.md)
  now releases full history before backend execution while retaining bounded
  retry context. Native probes and scheduler checks pass; the optimized local
  package is verified and installed as the WSL user CLI with rollback. Transient
  history reads, live allocation attribution and a long soak remain open.

- [Native Windows diagnostics](validation/beta-progress-2026-11-03/windows-memory-diagnostics.md)
  now include process working sets and preserve unknown CPU/load/collection
  readings. Native tests and typechecking pass. The [live central recheck](validation/beta-progress-2026-11-03/live-central-8443.md)
  still has failed asset GETs, a 401 API response and denied Windows SSH. Live
  pipeline attribution and memory profiling remain unavailable without access.

- [Windows updater key discovery](validation/beta-progress-2026-11-03/windows-updater-config.md)
  now uses the desktop profile when HOME is unset. Native tests and installer
  build pass; the closed local desktop test installation is updated with rollback.
  Trusted key/feed provisioning, signed update/restart and fresh-host acceptance
  are still required.

- The [current public recovery candidate](validation/beta-progress-2026-11-03/current-public-release.md)
  at `5e261e0e` includes worker factory reads and the streaming-ledger repair.
  Its optimized CLI is installed in WSL with rollback and fresh Windows
  integration reads. Actual isolated release installation
  and all 29 cold dashboard JavaScript chunks pass. The central remains undeployed;
  signed updates, target compatibility and authenticated live acceptance remain.

- [Bounded controller passes](validation/beta-progress-2026-11-03/controller-pass-turnover.md)
  stop sustained queues from keeping completed pass results alive indefinitely.
  Refill, pressure, shutdown and cross-pass work eligibility have local evidence.
  This bounds record count, not total memory or execution time; live OOM
  attribution and a long soak are still required.

- [Streaming event reads](validation/beta-progress-2026-11-03/event-reader-memory.md)
  remove an additional whole-journal allocation and bound limited feed queries.
  A child-memory regression fails before the repair and passes afterward;
  controller recovery/history semantics and the combined build pass. The full
  daemon's live allocation source and multi-hour stability remain unverified.

- [Bounded Rust output capture](validation/beta-progress-2026-11-03/rust-output-memory.md)
  now limits a reproduced unbounded live line buffer and coalesces progress.
  Incomplete reviewer output cannot supply approval. Native runner/workflow/CLI
  and refill checks, formatting and Clippy passed; the matched private assembly
  passed 1,093 server and 15 mock cases. Confirmed Linux loop OOMs in #1681 still
  need live allocation profiling and a multi-hour idle/dispatch soak. The local
  copier probe does not establish the live root cause; Mac #1680 stays deferred.

- [Exact factory account routing](validation/beta-progress-2026-11-03/factory-account-routing.md)
  now carries node/account/model selection through the real CLI and ledger,
  refuses older workers that cannot preserve it, and acknowledges submission
  with safe identical retries. Local CLI, fleet, server and Windows browser
  checks passed. Real-provider execution and live central/worker deployment
  remain unverified. The matched recovery ZIP and WSL user CLI now contain the
  feature; the live central and existing worker release were not upgraded.

- Windows x64 desktop `c1a55cf0` completed an actual current-user upgrade.
  Installed bytes and preserved settings were verified; its NSIS/MSI and
  matched combined bundle are saved locally. The user WSL CLI was subsequently
  updated to `16e9ecc6` with a rollback copy. Installed-CLI
  metadata and Mistral preflight passed. The app was left closed; current-build
  rendering and real sign-in remain unverified. The update started no worker
  service; an existing WSL worker resumed after the authorized distro restart
  and continues using its own separate release binary.
  Do not mix candidate manifests. This is unsigned
  local setup evidence, not signed-update, fresh-OS or live-server acceptance.

- Broad local shared-web checks passed at `0ee85993`: 160 Windows workflows,
  195 Windows component cases, 130 unit cases, typechecking and the remaining
  POSIX-only component case under WSL. These isolate the sample/API fixtures;
  live factory, real provider consent/billing and physical iOS acceptance remain
  required.

- The finishing branch now includes its previously missing merged factory and
  deployment-prefix fixes through `7848c117`. The combined 21-case real-CLI
  factory fixture, 32 source-structure checks, focused web checks and production
  asset-body smoke passed; see [merged factory checks](validation/beta-progress-2026-11-03/merged-factory-checks.md).
  The hosted-runner hang in #1662 remains unresolved, and these server/web
  outputs are not deployed to the live central.

- Native Windows now shows recovery controls when a loaded dashboard document
  stays blank because the app never mounts. Browser tests, all 27 native tests
  and rendering in an isolated failed-asset webview passed. Native settings-link
  activation remains unverified because Windows automation lacked input geometry;
  unfinished navigation now also has bounded native recovery; see
  [navigation timeout checks](validation/beta-progress-2026-11-03/desktop-navigation-timeout.md)
  for successful cases and the concurrent-startup limitation.

- New release bundles support isolated dependency installation from their own
  lockfile before replacing artifacts. Cold HTTP startup, moving the prepared
  output, source rebuild cleanup and failed-install preservation passed locally.
  Legacy bundles reject mismatched or unreadable lockfiles. The updated CLI and
  bundle still need deployment and acceptance on the live host.
- A complete Linux candidate at `1c0c38af` passed the actual optimized CLI's
  local-manifest install across dependency drift in a disposable checkout,
  preserving tracked checkout files and serving the installed dashboard.
  Its binaries require glibc 2.39; host compatibility remains unverified.
  MacBook work is explicitly deferred by the owner.
- The newer central recovery candidate at `d86a59b4` passed an actual optimized
  release install with Windows npm first on WSL's PATH, preserving checkout
  manifests and serving installed asset bodies. The WSL updater now selects
  Linux npm for that case. See [toolchain and recovery handoff](validation/beta-progress-2026-11-03/wsl-release-toolchain.md).
  The package still requires glibc 2.39 and is not deployed to the live host.

- Michael's controller status, route/claim state, and ledger are not available in this session. GitHub review comments establish recorded opinions, not why his running loop has not acted.
- Parasaur was confirmed at `https://hermesagent.tail82695.ts.net:8443`; the original HTTPS listener serves Hermes. Windows saved desktop connection URLs were corrected. The live central reports v0.2.0 and ready backend checks, but its referenced JavaScript/CSS GETs return HTTP 500, preventing dashboard rendering. Protected API reads return 401 until a central credential is supplied. Repair asset delivery and authenticate before accepting the live factory.
- Windows v0.3.0 completed the local per-user upgrade and navigation recovery
  checks described above. Signed updates, fresh-OS installation and live factory
  acceptance remain pending.
- Windows native provider commands now use the selected WSL node's CLI.
  A real Windows-to-WSL synthetic two-account save/list/create/bind/remove flow
  and transport limits passed; see [provider transport](validation/beta-progress-2026-11-03/windows-provider-transport.md).
  Actual provider authentication, per-account execution and billed usage still
  require acceptance. Windows Mistral browser sign-in is now implemented with
  native cookie isolation and staged WSL checks; see
  [Windows Mistral evidence](validation/beta-progress-2026-11-03/windows-mistral-sign-in.md).
  Real owner-driven sign-in and other providers' OAuth remain unverified.
  The subsequent [account execution check](validation/beta-progress-2026-11-03/windows-account-execution.md)
  verifies concurrent synthetic launches, rebind attribution and revocation at
  the Windows-to-WSL launcher boundary. Real provider execution remains pending.
- The shared model browser now remains usable when one provider never answers:
  independent results, configured defaults, timed reads, cancellation and Retry
  passed headless Windows checks. See [model-browser recovery](validation/beta-progress-2026-11-03/model-browser-recovery.md).
  This web change is local and has not been deployed to the central dashboard.
- Hosted enterprise acceptance requires the actual service and provisioned accounts.
- Workspace environment implementation now covers development application variables in interactive chat and exact factory checks. Local real-CLI/child-process validation passed with provider/state isolation and production exclusion. Other execution nodes retain their own configuration; a remote-node and real-provider workspace workflow still needs acceptance.
- iOS background delivery requires the configured APNs service, a signed app, and a physical device check.

Record the exact build, platform, commands, and live acceptance results before declaring beta ready. Keep this checklist pending while any required workflow remains unverified.

The [2026-11-04 scope audit](validation/beta-progress-2026-11-03/readiness-audit-2026-11-04.md)
maps every requested capability to evidence and remaining acceptance. It also
records the later named-account catalog repair; the recovery ZIP at `d86a59b4`
predates that repair.

Later local changes resolve [execution-node account discovery](validation/beta-progress-2026-11-03/node-local-chat-accounts.md)
and [factory diagnostic refresh/error states](validation/beta-progress-2026-11-03/factory-diagnostic-refresh.md).
Both have focused Windows evidence and remain undeployed. The older `d86a59b4`
recovery package predates them; the current `16e9ecc6` package below includes
them. Local package checks do not replace live factory acceptance.

The [enterprise integration record](validation/beta-progress-2026-11-03/enterprise-current-pair.md)
includes earlier Windows headless two-account invitation/removal checks.
The later [bridge shutdown repair](validation/beta-progress-2026-11-03/enterprise-bridge-stop.md)
was followed by [durable inference retry receipts](validation/beta-progress-2026-11-03/enterprise-inference-receipts.md).
The [inference-side consumer](validation/beta-progress-2026-11-03/enterprise-inference-consumer.md)
is retained with the new [compute process boundary](validation/beta-progress-2026-11-03/enterprise-compute-isolation.md)
and [claim-bound inference socket](validation/beta-progress-2026-11-03/enterprise-inference-relay.md)
at private `360ff0c6`, paired with public core `252d4b05`, including the
[startup authorization repair](validation/beta-progress-2026-11-03/enterprise-startup-authorization.md),
[shared capacity/native watchdog](validation/beta-progress-2026-11-03/enterprise-host-supervision.md),
[durable compute start/result delivery](validation/beta-progress-2026-11-03/enterprise-compute-recovery.md),
[bounded workspace/archive](validation/beta-progress-2026-11-03/enterprise-bounded-workspace.md),
[opt-in automatic queued compute](validation/beta-progress-2026-11-03/enterprise-compute-queue.md),
and [verified crash reconciliation](validation/beta-progress-2026-11-03/enterprise-compute-reconciliation.md).
[Saved-output controls](validation/beta-progress-2026-11-03/saved-output-controls.md)
now connect owner-only listing, bounded downloads and explicit reviewed discard
to shared Settings. Browser behavior and a full sample Settings workflow passed;
large-file saving in the installed Tauri app remains unverified.
The [hidden Windows native check](validation/beta-progress-2026-11-03/windows-native-output.md)
now verifies the existing stream path with real WebView2 filesystem objects:
20 MiB saving, corrupt-block rejection and cancellation preserve existing files.
Its `d531a72f` / `f322996b` assembly passes the production build; the interactive
picker and user-selected filesystem paths still require acceptance.
The exact combined build, SQL replay and real Auth/PostgREST/HTTP fixture-provider
checks passed. Seventy-seven dedicated WSL checks and sandboxed inference passed,
retaining the unchanged native host's earlier startup-crash evidence, including Node freeze, uncertain cleanup and lost
committed start/completion replies with preserved host input and bounded output.
Two queued fixture jobs now complete the sandbox/inference/archive transaction
with one provider attempt each. A real disposable consumer kill/restart now
recovers stopped work and empty storage without another provider attempt or an
invented backend exit.
[Receipt retention controls](validation/beta-progress-2026-11-03/journal-retention-controls.md)
now show the journal admission limit and require reviewed, owner-confirmed
cleanup with fresh exact-claim retirement proof. Interrupted removal preserves
durable evidence and can resume. The `5ef4458` / `189f01e3` pair passes 1,084
server and 15 mock cases plus typechecks, production builds and artifact guards;
unchanged production sources passed 89 dedicated WSL cases (one platform skip),
18 browser cases and a full Settings workflow. Final-pair real Auth/PostgREST
passed maintenance, retired-claim execution denial, queue/crash recovery and
authorization/relay checks.
[Workspace admission visibility](validation/beta-progress-2026-11-03/compute-admission-visibility.md)
now reports compute pauses separately from liveness, with independent expiry and
guidance visible to hardware members without dashboard access. The `4bca434` /
`f11b53c3` pair passes 1,087 server, 15 mock and 90 dedicated WSL cases (one
platform skip), plus typechecks/builds and five cloud Settings browser cases.
Real SQL/Auth/PostgREST passed admission reporting/expiry and the retained
queue, recovery, maintenance, authorization and relay checks.
[Retired-claim delivery reconciliation](validation/beta-progress-2026-11-03/compute-delivery-retirement.md)
now ends a reproduced endless retry with exact retirement proof, preserving local
and cloud outcomes and displaying delivery as unconfirmed. Owner cleanup still
requires fresh proof. Private `9b4ae6d` has production sources identical to the
`c013752` / `b0cd55e1` build, which passed 1,093 server, 15 mock, 96 dedicated WSL
cases (one platform skip), 19 browser cases and a full sample Settings workflow.
The corrected Auth driver passed real local/cloud result preservation,
retired-claim denial, reviewed cleanup and retained queue/recovery/relay checks.
[Bounded result replay and enrollment repair](validation/beta-progress-2026-11-03/compute-delivery-authorization.md)
now bound stalled delivery RPCs and reload same-origin credential repairs without
rerunning completed work. Private `3bfc79b` / public `819937bf` passed 1,099 server,
15 mock and 100 dedicated WSL cases (one platform skip), typechecks/builds and
real local SQL/Auth/PostgREST/relay checks. Hosted authorization and per-receipt
recovery UI remain open.
Hosted reporting, legacy/unknown state, capability-metadata
maintenance, installed desktop/hosted artifact acceptance, production runtime/client and
forge access, portable aliases, identity/ledger integration,
artifact publication, full shared-hardware acceptance, staged migration,
hosted rollout and a real provider-backed job remain unfinished.

A [current public recovery candidate](validation/beta-progress-2026-11-03/current-public-release.md)
at `16e9ecc6` now includes the later server/web fixes and exact factory account
routing. Its optimized release
install, full asset/range reads and Windows-to-WSL smoke checks passed. The user
WSL CLI is updated; the unchanged native desktop remains installed and closed.
The candidate requires glibc 2.39 and is unsigned, unpublished and undeployed.
