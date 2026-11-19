# Isolated inference connection — 2026-11-04

Follow-up: use the [startup authorization repair](enterprise-startup-authorization.md)
at private `5046d2c0` for the compute boundary. The earlier gate did not safely
handle controller death before authorization. Node startup still does not enable
automatic compute execution.

Verified private `841fc124fb7b4817674cebc7bef5d9e90270f638` with public core
`f7294e34f5684a9b4f7ac2cee43234062b98b24d`. An isolated process can now use a
claim/model-bound inference socket without gaining host network access or the
connector's device credentials. The automatic compute queue consumer is still
absent. This is local implementation evidence, not an enabled factory workflow.

## Implementation

The connector creates a private Unix socket and forwards only the two supported
inference routes to the existing durable cloud admission/poll RPCs. Its work ID,
claim ID, selected model and device identity are fixed by the trusted caller.
The job cannot choose another account, provider endpoint, model or arbitrary RPC.
The sandbox mounts only this socket; its host directory and credential files
remain absent. No host provider fallback exists.

Each job request requires a client-generated `Idempotency-Key` UUID. Lost
admission replies retry only the original key and body. Subsequent HTTP reads or
relay restarts use the SQL receipt, rather than a new provider call or local
replay cache. Missing keys, changed identities, unavailable models, revoked
access and malformed/oversized messages fail explicitly. Device credential
echoes, including JSON escapes, are withheld. Provider error responses retain
their status and JSON content type even when the request selected streaming.

Prompts are capped at 256 KiB, results at 1 MiB, connections/handlers at eight,
with upload, RPC and overall deadlines. Stop aborts and joins active RPCs and
removes the socket. An HTTP client disconnect does not invent a new request
identity or cancel a billable attempt that the cloud already accepted.

The existing inference worker and relay now share bounded RPC/response reading.
The worker's twelve focused regressions continue to pass.

## Exact-revision evidence

- Official private build: 1,027 server passes, 12 explicit skips, zero failures;
  13 mock passes; server/web typechecks and builds; private artifact/input and
  transformed browser-transport guards.
- Dedicated `npm run test:isolation`: ten real WSL sandbox checks passed; only
  the native unsupported-platform check skipped. This includes an actual
  isolated Python HTTP client using the mounted capability while host network,
  credential files and arbitrary RPC routes remain unavailable.
- Ten Unix HTTP relay tests cover lost admission, repeated request identities,
  validation before RPC, denial/conflict, bounded replies, credential echo,
  escaped credential echo, buffered streams, JSON provider errors and joined stop.
- Real disposable Auth/PostgREST/SQL, the actual relay and inference worker, and
  an HTTP fixture provider completed one end-to-end sandboxed request. Repeated
  reads inside the job and from the connector produced one new provider attempt.
  The exact claim was revalidated after namespace setup, before execution.
  Credential canaries stayed outside the job; its negative probes received only
  digests, not the actual device/provider values.
- SQL replay, concurrent receipt retry, active-provider revocation, HTTP and
  WebSocket relay, identity, entitlement and member-removal checks passed.
- Native Windows focused inference/platform checks passed during this pass;
  Linux socket/sandbox checks explicitly skipped there. Windows also passed the
  three WebSocket authentication tests.

An intermediate combined run exposed a pre-existing WebSocket expiry test race:
500 immediate callbacks allowed only about 14 ms for actual socket IO. Public
`f7294e34` waits for the close event with a real two-second failure deadline while
expiry uses mocked time. The corrected combined run passed. Authentication and
expiry policy were not changed.

The final source stayed clean at
`/home/khing/.cache/parasaur-enterprise-current.nmulrskh/source`. Its sibling
`build.log`, `isolation-verification.log` and `auth-verification.log` retain the
results; `.build/release.json` identifies both full revisions above.

## Remaining integration

The receipt protocol buffers event streams until completion. Real token
streaming and native CLI subscriptions are not implemented by this relay.
SDKs unable to supply stable per-request keys remain incompatible; they must not
silently use another account or paid API.

Implement the compute queue consumer, production dispatch/provider runtime and
its stable-key client, Git/forge access, portable aliases/skills, ledger identity,
durable reservation reconciliation, bounded workspace disk allocation, shared
local/cloud capacity and staged migration. Real nodes, subscriptions, hosted
configuration and factory acceptance remain open. No release, installed binary
or live host was updated. MacBook work remains deferred.
