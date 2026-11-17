# Exact factory account routing, 2026-11-04

The finishing pass found that chat could choose a named provider account but
Factory's Queue work form could send only a backend family. The CLI dispatch
command had no exact account argument. Choosing a model did not close that gap.

Queue work now offers execution node, node-local account and optional model
controls. Automatic fleet/configured routing remains available. Node or backend
changes clear the account and model; changing project/repository remounts the
form. Disabled, missing and known unauthenticated accounts cannot be selected.
Providers configured on healthy workers remain selectable when central does not
have that provider installed. Session cards retain the selected account and node.

The chosen identity travels through WebSocket and HTTP dispatch, fleet transport,
SessionManager and `gah dispatch --backend-instance`. Remote requests must pin a
node and a concrete backend. A worker must advertise exact-account support;
older workers are rejected rather than being allowed to ignore the new field.
The CLI resolves only that declared account, retains its execution identity,
checks model allowlists/availability/approval, and never falls back to a sibling.
Sibling billing and quota metadata is not inherited by the selected account.
Node-local account identity is part of the saved lease's work key. Legacy lease
keys remain readable, and conflicting request receipts are rejected.

Submission stays pending until a matching server reply or reconnect snapshot
acknowledges it. A 15-second timeout says the outcome is unknown, freezes the
selection and offers a retry of the identical request ID and arguments. A direct
reply also identifies its request when it joins existing work whose original
request ID differs. Server failures remain visible and release the form.
The shared request-ID helper supports plain HTTP LAN browsers without randomUUID.

The sample control plane now simulates factory start/stop and idempotent account
receipts in memory. Its existing sample-data banner remains the authority: this
does not launch a provider or demonstrate a real factory lifecycle.

## Verification

- `cargo test --lib routing::`: 114 passed, including six new exact-account
  cases for identity, disabled/unknown/incompatible accounts, allowlists,
  approval and model inference. No sibling fallback or metadata borrowing.
- `cargo test --test gah_cli dispatch::operator_pins`: three passed. The new
  real CLI fixture runs two validation attempts through the account's own
  executable and isolated Claude state, checks both attempt identities and
  final ledger account/quota attribution, and verifies the original config is
  unchanged. External provider/forge commands are isolated fixtures.
- `cargo test --lib cli::capabilities`: nine passed. Capability artifacts were
  regenerated from the Rust-owned schema with the optional backendInstance field.
- `cargo fmt -- --check` and
  `cargo clippy --all-targets --all-features -- -D warnings`: passed under WSL.
- All 32 source-structure checks passed using the real checkout's Git index
  against the isolated Linux source. The exact account field pushed three
  existing files past the 1,500-line limit; account tests, claim control-record
  tests and initial route-request construction were split into focused modules.
  The source-size baseline was not changed. All 58 claim tests passed after the
  move; routing and real CLI retry checks were rerun and passed as well.
- `npm run typecheck`, server production build and shared-web production build:
  passed. Full Linux server suite: 1,001 passed, one skipped. The later mock-only
  change passed its own final typecheck and all 13 mock tests on Windows.
- Focused fleet tests: all 23 passed, including actual socket serialization
  over both configured HTTP/HTTPS agents, old-worker rejection, unavailable
  account/model refusal, retry mismatch refusal and restart recovery.
- SessionManager tests: all 13 passed. Exact identity and receipt correlation
  survive failed dispatch without a fallback. The full Linux suite also checks
  the account argument at the actual child-process argv boundary.
- Windows headless Chromium: four factory account workflows plus the existing
  review-hold workflow passed. Covers a provider available only on a worker,
  account/model clearing on node change, uncertain identical retry, joined-work
  acknowledgements, visible failure, old-central compatibility and a real
  stateful sample-server receipt. Desktop and 390px layouts were inspected;
  controls fit and input/select/dispatch targets are at least 44px high.
- Seven existing provider/model picker component tests passed after the
  backend-only picker was changed to close on selection.

## Acceptance still open

This is local implementation and fixture evidence. It does not prove real
provider consent, billing, remote credential isolation or a live issue lifecycle.
The implementation commit was followed by a [matched recovery package and WSL
CLI update](current-public-release.md). The unchanged native desktop was retained.
The older immutable recovery ZIP at b0ea279f predates this feature. Central and
workers still need deployment before named factory accounts are available live.
Enterprise workspace aliases/isolation/migration, signed updates and physical
iPhone acceptance remain separate unfinished requirements in BETA_READINESS.md.
