# Shared web workflow sweep

Verified the public working tree at `0ee85993` on Windows without production
configuration, provider execution or live-node access. The worktree was clean
before and after the checks. This pass found no behavior failure requiring a
code change.

| Check | Result | Scope |
| --- | --- | --- |
| `npm run e2e --workspace=apps/web` | 160 passed, 3.5 minutes | Chromium and the isolated stateful sample control plane; web port 59691 and mock port 59692 |
| `npm run test:component --workspace=apps/web -- --workers=1` | 195 passed, one Windows skip, 1.1 minutes | All 47 component spec files |
| `npm run test:unit --workspace=apps/web` | 130 passed across 16 suites | API client, stores, output buffers, parsing, labels, layout and identity helpers |
| `npm run --workspace=apps/web typecheck` | Passed | Shared web TypeScript |
| POSIX-only component case under WSL | One passed | Generated installer command executed against a fake curl/installer and synthetic input |

Both Windows Playwright result files reported `status: passed` with no failed
tests. E2E's owned mock/Vite listeners and the Windows component-test listener
were gone afterward. No fixture server was kept running for this pass.

## What the workflow coverage establishes

- Factory claim refusal/admission/missing evidence, paused review receipts,
  work drawers, holds, controller decisions, paid-route approval boundaries,
  and queue/session navigation behave correctly with the fixture responses.
- Chat restores interrupted streams and pending permissions, accepts steering
  and cancellation, and rejects stale responses after project changes.
- Project selection, profile editing, environment-source status, exact command
  lists and node selection persist through their sample API flows.
- Multiple named accounts remain distinct in configuration and usage views.
  Delayed login results, cancellation, unavailable observations, quota freshness,
  independent worker allowances and duplicate verified identities have explicit
  component assertions.
- Git issue/PR navigation, review editing, publication requests and writable
  checkout controls call the fixture APIs correctly. These tests do not publish
  to a real forge or establish preservation of a real checkout by themselves.
- Authentication recovery, browser pairing/revocation, notification navigation,
  sample labeling, desktop-only controls and responsive route layouts pass.
  Simulated iOS bridge checks are browser tests, not physical-device acceptance.

The Windows skip is intentional in
`ColocatedProviderSection.spec.tsx`: POSIX PATH fixtures cannot intercept a
Windows bash command forwarded into WSL. The same case passed with Linux Node
under WSL after copying the current spec, Settings source, node-command builder
and component config with canonical LF endings. Its generated command used a
local curl stub and canary strings, not a real installer or API credential.

Vite emitted WebSocket `ECONNABORTED` proxy messages while test contexts closed.
The runners exited zero; these messages did not represent failed assertions.
The component build also emitted its existing static/dynamic React import
warning. No production logging behavior was changed to suppress either output.

## Acceptance limits

This is broad local browser evidence for the current shared UI. It does not
prove that Michael's controller advances eligible work, that the live dashboard
serves its assets, that real provider consent succeeds for multiple accounts,
that measured usage reconciles with provider billing, that a signed desktop
update installs, or that a locked physical iPhone receives its Live Activity.
Those remain in [Beta acceptance](../../BETA_READINESS.md). MacBook/live-host
work remains explicitly deferred by the owner; this pass requested no secrets
and changed no remote service, issue gate or live job.
