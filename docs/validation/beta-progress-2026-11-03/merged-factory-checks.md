# Reconcile merged factory and deployment-prefix fixes

On 2026-11-04 the finishing branch did not contain five commits already in
its existing `origin/main` ref (`35e4dff0`). The branch now includes that ref
through merge `7848c117`, preserving the beta work. No push, PR, live claim,
review gate or controller job was changed.

The reviewed refill fix from PR #1675 was first restored as `4efcc624`.
It combines a fake worker's call numbering and active count under one lock,
isolates test memory calls from the host gateway, and prints the owned process
tree when a controller fixture hangs. These are test fixes, not proof that
Michael's live controller was stuck for the same reason. A baseline run was
explicitly terminated after confirming it lacked gateway isolation; its partial
results are not a passing run.

The remaining integrated commits restore quota/boost/pressure integration
coverage (#1672), opt-in routine node-offline alerts (#1674), deployment-prefix
handling for API/WebSocket traffic (#1677), and prefixed notification navigation
(#1676). The latter prevents notification clicks from leaving a deployment such
as `/parasaur` for the origin root. Security and required-action alerts remain
enabled by default.

## Verification

- Restored refill target before the full merge: 19 passed in 113.09 seconds.
- Combined `controller_refill_regressions` target: 21 passed in 162.73 seconds
  under this Windows machine's Ubuntu-24.04 WSL distro. This includes worker
  scaling from quota, manual boost, expiry, weaker allowance, and the lower
  node-pressure admission limit, alongside mid-batch review and worktree
  isolation. Commands used `--locked --offline`, persistent
  `CARGO_TARGET_DIR=/home/khing/.cache/parasaur-beta-target`, and an explicit
  `CARGO_BIN_EXE_gah` pointing to its debug CLI.
- Root `source_structure` target: 32 passed. The first attempt had six shell
  failures from the Windows checkout's CRLF `scripts/install-linux.sh`.
  Restoring that file to the existing `.gitattributes` LF requirement resolved
  them; no shell logic or assertions changed.
- Contracts/shared builds, shared-web typechecking, server build, and production
  shared-web build passed.
- Focused web unit tests: 13 passed across API client, network path/URL guards,
  and WebSocket URL handling.
- All three activity-preference tests passed under WSL against the newly
  compiled JavaScript, including Unix permission checks and persisted defaults.
- A headless HTTP smoke test started the compiled server with isolated identity
  storage and the current production web build. Dashboard HTML, both referenced
  JS/CSS bodies, and byte-range GETs passed; asset bytes matched the build files.
  The owned listener closed afterward. No browser or provider was launched.

Issue #1662 remains unresolved: the intermittent hosted-runner hang was not
reproduced, and the under-60-second, three-hosted-run acceptance criterion was
not met. Do not close it from these results. The local production asset success
also does not repair the live central's asset GET 500s or establish live factory
progress. The installed native desktop/WSL CLI candidate remains `c1a55cf0`;
these newly integrated server/web outputs are local and undeployed.
