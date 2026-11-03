# Beta finishing pass: local validation

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

## Checks

- Root `npm run typecheck`: passed across contracts, shared, server, mock, and web.
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

Structure checks also require a real Git index and baseline history. The Linux snapshot now has its own shallow metadata copied from this branch and the existing `origin/main` ref; its tracked shell/executable sources were normalized to their canonical LF form. It does not share writable Git metadata with the Windows workspace.

The Windows test application is the installed v0.3.0 Tauri shell loading the current local UI and sample backend. The CLI was built and tested inside WSL. This does not establish that a new Windows installer has been built, that a release has been published, or that the deployed factory is running these changes.

Named-account browser sign-in currently operates on the connected central node's configured accounts. Fleet login-health rows still retain their existing manual named-instance guidance. Real provider account consent, saved-credential replacement, and remote-worker named login remain acceptance work. The local sample backend was deliberately restarted to load its new explicit sign-in response and was checked through the same port-38000 Vite proxy used by the native test shell.

Factory environment sources retain their existing runtime scope: development is the default, and production requires `gah dispatch --prod`. Interactive chat does not yet apply these profile source settings. Factory check editing now uses lossless command transport and an owner-only view, but other execution nodes retain their own configuration. Full workspace/environment acceptance remains pending.

The saved desktop central origin answered health checks but returned 404 for the dashboard and `/api/info`. The actual Parasaur address and Michael's runtime state remain unavailable; no live claim, review gate, retry budget, or deployed configuration was changed.

The owner confirmed the saved tailnet hostname is intended to host both Hermes and Parasaur. A fresh request identified its current HTTPS service as Python aiohttp, `platform: hermes-agent`, version 0.21.4; the host was online in Tailscale. Direct ports 3773 and 3774 did not accept these Windows probes. SSH as the owner's specified `khing` user reached the host but rejected this Windows key. MacBook relay tools were unavailable in this session. A read-only routing/service check from the MacBook is pending before assigning Parasaur a separate HTTPS port; Hermes routing has not been changed.
