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

- Windows x64 desktop `c1a55cf0` completed an actual current-user upgrade.
  Installed bytes and preserved settings were verified; its NSIS/MSI and
  matched combined bundle are saved locally. The user WSL CLI was subsequently
  updated to `d86a59b4` with a rollback copy. Installed-CLI
  metadata and Mistral preflight passed. The app was left closed; current-build
  rendering and real sign-in remain unverified. No worker service was started.
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
Both have focused Windows evidence and remain undeployed; the installed desktop
and immutable recovery package predate them. They do not replace live factory
acceptance.
