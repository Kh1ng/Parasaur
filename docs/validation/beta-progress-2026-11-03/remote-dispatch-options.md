# Remote dispatch preserves execution controls

The owned central-to-Windows preview exposed a worker transport defect:
`fleetDispatch` serialized `dryRun`, `retries`, `allowDraftFail`, `prod`,
`allowUnknownRedBaseline` and `escalate`, but the WebSocket contract and handler
omitted them. A requested preview could therefore become a normal dispatch.
The diagnostic used a nonexistent branch and no real issue. Its original
session was explicitly cancelled, with worker confirmation, before deployment.

Public `65abf86a` shares the execution-control type across the transport,
session manager and CLI adapter. REST and WebSocket boundaries use one strict
allowlist parser. Boolean strings, nulls and invalid retry counts are rejected
before dispatch; explicit false and retry zero survive. Unrelated network fields
cannot select an environment or CLI configuration file.

New workers advertise `dispatch_execution_options`. Fleet selection excludes
older workers when a meaningful override is requested. An explicit pin to an
unsupported worker fails with `DISPATCH_OPTIONS_UNSUPPORTED`, without a local
fallback. Existing requests using defaults remain compatible. A shared parser
and capability check were chosen over independent endpoint coercions because
coercion was precisely how a requested preview silently became execution.

## Verification

- Contracts build, server typecheck and diff whitespace check passed.
- The isolated Linux dispatch, WebSocket, HTTP, session and CLI suite passed
  107 cases. A further CLI/registry run passed 57 cases, including actual fake
  executable argv capture for all execution flags and zero retries.
- Private `d039ae3` pins the public fix. Its combined build passed 1,122 server
  cases with 38 explicit skips, 16 mock cases, production builds and private
  input, artifact and browser credential guards. Log:
  `/home/khing/.cache/parasaur-enterprise-current.tft00lgf/build.log`.
- Targeted worker and central builds reproduced every installed module preimage
  before applying this patch. Existing notification, claims and lightweight
  welcome changes were preserved, as were the legacy GitLab handoff and CLI.

The initial native Windows broad test run encountered existing POSIX fixture
limitations; authoritative integration checks ran in an isolated WSL checkout
with Linux dependencies. No Windows dependencies were replaced with Linux ones.

## Live acceptance, November 6

The Windows WSL worker was idle and updated at 11:26 UTC, PID 178252 to 230925.
The central API passed its all-session idle check and updated at 11:27 UTC,
PID 628728 to 687517. Both retained node identity, config, credentials and CLI
hashes. Central retained its chat index. Rollback copies are held in the
respective private repair directories under `dispatch-options-65abf86a`.

A new owned request, `factory-dry-run-ced0884a124043a0974b8c52a90459b2`,
returned HTTP 200 and a terminal stopped session on pinned worker
`011f5dca-4228-4aff-a7f0-d12953d86c0a`, without timing out. It selected the
configured OpenCode/Mistral route, a diagnostic branch and no real issue;
`manualWorker` was not requested. The subsequent worker roster had no active
sessions. The API does not expose a numeric session exit code; no value is
invented in the evidence.

Native controller PID 666974 and observer PID 667504 were unchanged. At
11:27:54 UTC the existing ten-hour memory sampler was running with 36 samples
and zero gaps. This remains an incomplete soak. Queue item #1525 continues to
report `node_capacity_deferred`; this patch does not add automatic native-loop
handoff, merge independent controller histories, lower admission limits or
establish end-to-end factory completion.

Metadata-only deployment, dry-run and observation reports are retained under
ignored `artifacts/central-live-20261105/`. Reproducible targeted build archives
and per-module hashes are under `artifacts/parasaur-dispatch-options-65abf86a/`.
