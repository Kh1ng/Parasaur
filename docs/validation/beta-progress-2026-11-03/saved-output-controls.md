# Saved-output review and retention controls — 2026-11-04

Verified private `360ff0c61dc8a05a345d3803b54447c8082b7421` with public core
`252d4b05aed76121bb70bf1e44c081624148642d`. This completes API and shared
Settings integration for the [storage primitives](compute-output-storage.md).
It is local candidate evidence, not installed-desktop or hosted acceptance.

## Operator behavior

Settings → General → Saved output shows this machine's reserved archive slots,
per-job bound and work identity. Active jobs, retained output and unresolved
cleanup remain distinct. Unverified archive sizes remain unknown, incomplete
uploads are labeled, and unrecognized storage entries explain blocked admission.
The section is included in Settings search.

Save output verifies fixed 1 MiB blocks through the existing authenticated
dashboard transport. A supported browser file picker streams verified blocks to
disk; the bounded fallback permits at most 16 MiB. Cancellation aborts the local
writer and retains the server copy. Neither route nor UI extracts archives.

Discard first obtains the observed content hash, then requires a separate
unchecked confirmation. The server requires owner access, a durable mutation
key, the selected slot incarnation, matching reviewed bytes, stopped-producer
evidence and an execution result. Unknown cleanup, links and unfamiliar files
remain retained. Stale/concurrent removal cannot delete a reused slot. A lost or
failed removal response clears the selection and requires a fresh list and
confirmation rather than automatically issuing another mutation key.

Changing machine or credentials cancels old requests and clears confirmation.
Catalog/block requests have 15-second deadlines; verification/removal requests
have 60-second deadlines. A hung catalog releases Loading and permits retry.
Unconfigured nodes explicitly report unavailable controls, including workers.

The public server supplies a trusted owner-only extension namespace after API
authentication and before worker role filtering. Paired devices cannot list,
download or discard node archives. Responses expose no host paths or process
identity records. Discard shares the durable administrative audit/replay guard.

The sample server implements only simulated partial output, in-memory discard,
reset and stale-selection rejection. Adding the UI initially failed its existing
API-coverage check; the missing fixture route was implemented rather than exempted.

## Exact-pair verification

Retained source: `/home/khing/.cache/parasaur-enterprise-current.2_6_7vgc/source`.
Sibling logs record:

- Combined production build: **1,072 server cases passed**, 37 explicit skips,
  zero failures; **14 sample-server cases passed**. Server/web typechecks,
  production builds, build-input/artifact guards and transformed transport checks passed.
- **11 browser behavior cases passed** on the assembled pair. These cover
  explicit confirmation/action keys, uncertain removal, authenticated downloads,
  corrupt/oversized blocks, bounded streaming, writer cancellation, target changes,
  unavailable/owner-only servers, hung catalog recovery and admission blockers.
  The separate desktop/390px render and keyboard-focus case passed in the scoped
  verification copy; its component source matches the assembled pair after line
  ending normalization. Desktop/mobile layouts and confirmation were inspected.
- **One full Settings workflow passed** against the actual sample server and
  Vite proxy: review, confirm, discard, empty list, reset and refresh.
- Dedicated WSL isolation checks: **77 passed**, one platform-only skip, zero failures.
- Disposable SQL replay and real Auth/PostgREST checks passed for ordinary queued
  jobs, bounded archives, lost start/result replies, consumer crash/restart,
  inference receipt replay, entitlement/removal, recipient isolation and HTTP/WS relay.

Logs: `build.log`, `output-browser-verification.log`,
`output-settings-verification.log`, `isolation-verification.log`,
`auth-verification.log`. The retained debug host's SHA-256 remains
`ac62e3cc98730f2e8839d7d79b867933e342b95b18a5f3f9cfd0708edaa1cf2e`;
its Rust/Cargo source match was checked against this core pin.
The final user-manager check found no remaining `parasaur-job-*.scope` units.

## Acceptance still required

Large-file saving in the installed Windows Tauri app, real hosted-owner use,
deployment/update acceptance, richer job/profile facts and artifact publication
remain unverified or unfinished. Browsers without a streaming file picker retain
the explicit 16 MiB fallback ceiling. Journal and orphan capability-metadata
maintenance are separate unfinished work; freeing archive storage does not clear
journal retention. No real user output was removed, provider called, installed
release replaced or production service enabled by this verification.
