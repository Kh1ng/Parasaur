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

## Checks

- Root `npm run typecheck`: passed across contracts, shared, server, mock, and web.
- Web production build: passed.
- Full Windows frontend unit suite: 125 passed after the project-isolation changes.
- Focused Windows component suites: connection recovery, work drawer, blocker actions, profile configuration, and account creation passed.
- Windows browser flows: factory environment/history, profile settings and project switching, and live account status passed, including narrow-screen checks.
- Latest Factory browser run: six passed, including searching a paused-review explanation and opening its details. Desktop and 390px screenshots were inspected. Login repair and repository CLI installation component suites: five passed.
- Full Linux server integration run under Ubuntu/WSL: 973 passed, one opt-in test skipped, zero failures. The later per-account probe handling passed its two focused server tests.
- Mock control plane suite: nine passed.
- The opt-in paid-route HTTP test was exercised separately against the actual Rust CLI and an isolated ledger: five Rust/support tests passed.
- Execution identity, AGY pool identity, and usage telemetry regression targets: 28 passed under WSL.
- Native Windows preview-proxy tests: 14 passed.
- OpenAPI YAML parsing and `git diff --check`: passed.

## Platform notes

The full server fixture suite requires POSIX executables and now refuses native Windows before running. Validate it inside a Linux/WSL checkout with separate Linux dependencies. The WSL test snapshot used canonical LF fixture scripts; `.gitattributes` now preserves executable-compatible line endings for future checkouts.

The Windows test application is the installed v0.3.0 Tauri shell loading the current local UI and sample backend. The CLI was built and tested inside WSL. This does not establish that a new Windows installer has been built, that a release has been published, or that the deployed factory is running these changes.

The saved desktop central origin answered health checks but returned 404 for the dashboard and `/api/info`. The actual Parasaur address and Michael's runtime state remain unavailable; no live claim, review gate, retry budget, or deployed configuration was changed.
