# Central controller native dispatch

## Real controller acceptance and recovery

Normal dispatch, a lost reservation reply and a lost activation reply now pass
through the real optimized Rust controller, central producer/broker, authenticated
worker HTTP API, fresh Rust work/route checks, default systemd admission, actual
backend fixture, canonical receiver and acknowledged capacity release. Each case
runs one backend attempt, records one successful terminal result, independently
proves the kernel lease free, preserves unknown actual cost, survives central
reload and leaves the completed work undispatched on the next controller cycle.
The managed-loop status observation, forge and backend are controlled fixtures;
no paid model or production service participates.

This acceptance found two defects missed by component tests:

- A complete run binding was serialized into the four-field worker catalog
  request. The worker correctly refused it, so fresh authority denied all starts.
  Transport now explicitly selects the four wire fields; its HTTP regression
  passes both a minimal identity and the complete binding used by real authority.
- A lost reservation acknowledgement left a dormant canonical hold. The next
  controller cycle excluded that work and reported its sole intake slot full,
  leaving no path to activation. The controller now observes canonically held
  dormant reservations and activates their original identity after live local
  ownership clears. Observation alone does not activate a run. Paused factories,
  live local owners and mismatched bindings are refused by focused tests.

The baseline failure is retained under
`/home/khing/.cache/parasaur-controller-http.717qSf`. Passing cases are
`parasaur-controller-http.qWVnZU` (lost reservation),
`parasaur-controller-http.zUC0io` (normal), and
`parasaur-controller-http.kSbabG` (lost activation), under the same cache root.
The compact report is `artifacts/central-live-20261105/native-controller-http.json`.
Reproduce with `scripts/tests/native_controller_http.mjs <API dist> <gah>` and
optional final mode `normal`, `lost-reservation`, or `lost-activation`.

Accepted public source is `a0fe1f1861377f83cedcea64a82d1d6e88b2c04d`, private
source `ea3048f5ad0e07f961a2e8afe5f9bd249f03b591`. The API/shared-web checkout is
`/home/khing/.cache/parasaur-enterprise-current.kp7ucmbs/source`; all 1,235 server
tests and 17 mock tests pass with 38 existing server skips. Production builds,
private input/release, credential-route and cloud-transport checks pass. Linux
checks pass 55 handoff, 578 dispatch, 76 provider, three CLI and 12 capability
tests, formatting and all-target/all-feature Clippy with warnings denied.
The 60 focused server tests and server typecheck pass.

The matching optimized CLI is
`artifacts/parasaur-native-context-a0fe1f18/gah-linux-x86_64`, SHA-256
`b29fa6e06d0a0a4cb4170c26773c18dbf1349a15a1e521b1350ec4c83f2b3b5f`.
Its 629 Rust/Cargo inputs match the source archive; release compilation,
native-context and zero-spend-ceiling production-binary smokes pass. It requires
glibc 2.39. Nothing is installed and real factory rollout remains pending the
provider-assignee, PM artifact and uncertain-start lifecycle work below.

## Initial controller implementation

The controller now prepares remote work through the central API instead of
requiring the selected provider executable on the controller. Each worker exposes
an authenticated account catalog containing readiness and supported models, with
no credential values or executable paths. Central applies its ordinary routing
rules, exact account selection, history, approvals and budgets to that catalog.
Unknown authentication readiness remains unknown.

Preparation reserves canonical work ownership without staging or starting the
worker. Only after receiving that durable reservation does the controller release
its own local claim and activate the original run. A lost reservation reply cannot
authorize activation; a lost activation reply retains the remote identity and
reports unconfirmed activation. Neither case authorizes a local backend fallback.

A private per-work index recovers a completed preparation after a lost reply
without another history copy. Partial preparations remain inspectable and refuse
replacement. Preparation is serialized to bound transient history memory.
Scheduling includes native claims in intake counts and excludes their work from
the selectable queue. Fresh start authority uses that same canonical claim scope,
excluding the run being authorized so its own reservation cannot block it.

Focused verification includes a real local HTTP controller test for successful
claim transfer, dropped reservation and activation replies, and a wrong-work
reservation. The server suite covers dormant reservations across reload, explicit
activation, worker selection, account metadata boundaries, partial preparation and
lost-reply recovery. These checks use controlled credentials and do not dispatch
production issues or prove a production controller-to-worker run.

Verification: 55 native handoff tests, 577 dispatch tests (including the three
controller native tests), 76 provider tests, three CLI handoff tests and 12
capability tests pass under Linux/WSL. Formatting and all-target/all-feature
Clippy with warnings denied pass. Contract/server typechecks and all 60 focused
server handoff, producer, transport and context tests pass. The shared review
account-pin rule also has a regression test for backend/model substitution.

The matched build uses public `36430bdde56128f7aca8febaa9c6d9cd89c3c21b`
and private `9d9cb7cc5dd05077be4437bef1c32084a2f81bd5`. All 1,235 server
tests pass with 38 existing skips; all 17 mock tests pass. Server and shared-web
production builds, private input/release guards, credential-route checks and
cloud transport checks pass. Its isolated checkout is
`/home/khing/.cache/parasaur-enterprise-current.wlk4ak_w/source`, with evidence
in the adjacent `build.log`.

The optimized Linux CLI is
`artifacts/parasaur-native-context-36430bdd/gah-linux-x86_64`, SHA-256
`675fce742ccf12f2ceac9e907a55aa1b3b1c1b60b8df842de8d58535dc52f9f7`.
All 629 Rust/Cargo inputs match that public source archive. Release compilation,
native-context verification and zero-spend-ceiling production-binary smokes pass.
The artifact requires glibc 2.39 and remains uninstalled.

Deployment remains pending. Provider-assignee claim renewal, remote PM artifact
publication and uncertain-start reconciliation without a terminal ledger receipt
remain incomplete. Existing worker memory/capacity guards are unchanged. Real
end-to-end acceptance must use the matched newly built API and Linux CLI before
installing this controller path.
