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

- Michael's controller status, route/claim state, and ledger are not available in this session. GitHub review comments establish recorded opinions, not why his running loop has not acted.
- The desktop's saved central origin answers `/health`, but its dashboard and `/api/info` return 404. Identify the actual Parasaur address before using it for live acceptance.
- Windows native smoke testing currently uses the v0.3.0 release shell with the current UI served locally and a sample control plane. It is not a newly packaged release or a live factory test.
- Native compilation on this Windows host requires the Visual C++ toolchain; its attempted installation was canceled at the administrator prompt.
- Hosted enterprise acceptance requires the actual service and provisioned accounts.
- Workspace environment acceptance remains incomplete: Profile can configure factory file sources and exact validation/auto-fix commands on the connected node, but interactive chat does not yet apply the profile environment sources and other nodes retain their own configuration.
- iOS background delivery requires the configured APNs service, a signed app, and a physical device check.

Record the exact build, platform, commands, and live acceptance results before declaring beta ready. Keep this checklist pending while any required workflow remains unverified.
