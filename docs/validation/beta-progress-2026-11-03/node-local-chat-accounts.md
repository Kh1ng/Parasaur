# Node-local chat accounts — Windows, 2026-11-04

The new-chat launcher previously listed central backend instances even when the
selected execution node was a worker. Changing nodes could retain a central
account identifier. Identical aliases on different machines do not establish
account identity. Existing conversation labels also came from central config.

Chat account discovery now resolves the selected project/runner/node through
the existing authenticated worker transport. Workers read their own profile
configuration and return account metadata only: identifier, runner, label,
enabled state, and nullable executable/auth readiness. Listing does not start a
provider or login process. Repository, provider-origin and node checks precede
the read; transport replies are bounded and validated, with private fields
discarded. Unknown readiness remains unknown.

The launcher scopes account choices by project, runner and node. Switching
nodes immediately removes the prior choice, cancels the old request and blocks
creation until discovery succeeds. Reads have a 15-second deadline, visible
failure and Retry. A late response cannot replace the current node's choices.
Existing sessions obtain account labels from their execution node too.

This uses a new worker action and central endpoint. Update central and workers
together: older workers fail explicitly rather than falling back to central
accounts. This change is local and undeployed. The installed desktop and the
immutable `d86a59b4` recovery ZIP do not contain it.

## Evidence

- Web, server, mock and contracts typechecks/builds passed; OpenAPI parsed.
- Fourteen focused server tests passed after restarting Ubuntu-24.04: metadata
  projection, transport validation/auth/identity checks, and the existing real
  central-to-two-worker coordinator fixture. The latter verifies that the same
  alias resolves to different node labels, a failing node does not cause a
  central fallback, and account reads do not run an agent. Its existing chat,
  workspace carry, archive and source-session checks remain passing.
- Fourteen launcher/picker component checks passed, including immediate clearing,
  lookup failure/retry, late-response cancellation, submission identity and
  mobile account/Retry touch targets. Desktop and mobile captures were inspected.
- Seven model-picker checks, seven API client checks, twelve mock tests and
  sixteen authenticated/mock chat end-to-end checks passed.

These checks use disposable fixtures and injected adapters. They establish local
behavior, not real provider consent, billing or live-worker acceptance. Windows
work remains headless; no Mac or live service was changed.
