# Inference-side cloud consumer, 2026-11-04

Private `0dc0142e2ff067521b20a5eca584c2323cc45b38`, paired with public core
`16e9ecc6f5ca8bd5a0db16c64574d1b6bde8dc17`, now implements the inference
half of cloud execution. An enrolled node can consume an authorized request,
call its explicitly selected OpenAI-compatible API resource with a node-local
credential, and deliver the result through the existing durable receipt RPC.
The compute-side queue consumer and shared-hardware isolation remain missing.

## Behavior and boundaries

This is opt-in through a private local resource configuration. The normal node
launcher validates and resolves the enrolled-device and inference-config paths
before changing its child working directory. No configuration was enabled on
the owner's installed node or live central during this pass.

The cloud request cannot supply an endpoint or credential source. Model/path
checks, current device/work/resource authorization and the original request
deadline gate execution. Active calls recheck permission and abort if it is lost
or cannot be verified. Unsupported models do not reach a provider. Redirects
are refused, response/RPC bodies are bounded, and recognized local credentials
in a response cause it to be withheld. Request IDs permit local receipt
correlation without logging work payloads or keys.

Each claimed request has one provider POST attempt. Only result delivery retries,
using the same request/claim IDs and response. Stop aborts and joins owned RPC,
provider and response-body work; late claims cannot start a call. The consumer
handles one request at a time, with jittered idle polling and bounded failure
backoff. Provider text/event streams are buffered within the existing response
bound; incremental streaming and wake notifications remain TICKET-248 work.

The additive deadline migration returns the request's existing expiry in a
claim. It does not reset that expiry or weaken the earlier durable receipt,
resource grant, memory or concurrency checks. No provider login files are read
or transported. Native subscriptions are not assumed to be portable.

## Evidence

- Twelve focused consumer checks passed on Windows and again in the assembled
  Linux suite: private configuration validation, exact endpoint/key forwarding,
  successful results, lost delivery replies, oversized responses, credential
  echoes, redirects, unavailable models/keys, late claims, request/body shutdown
  cleanup and permission revocation.
- The final clean private source passed its complete combined production build:
  **1,017 server tests passed, one opt-in skip, no failures**, plus **13 mock
  tests**, server/web typechecks/builds, artifact/input guards and transformed
  cloud transport checks. The release manifest records both revisions above.
- All SQL migrations replayed twice and the full authorization suite passed,
  including an assertion that claiming does not reset the request deadline.
- The real disposable Auth/JWT, edge and PostgREST fixture drove this actual
  consumer into a loopback HTTP provider. The selected provider key reached
  only that provider; its model response and usage fields reached the compute
  requester unchanged. Eight concurrent request retries retained one receipt
  and produced no second provider call. Completion removed private records.
- A second explicit job held the provider's HTTP response open. Revoking its
  compute permission caused the consumer's authorization probe to abort the
  connection. Renewal refused the job and cleaned its inference records.
  The provider observed exactly one call for each of the two explicit requests.
- Existing real HTTP/WebSocket relay, cancellation, invitation, entitlement,
  recipient-isolation and membership-removal checks also passed. The final
  private checkout remained clean and owned fixture services were confirmed
  stopped.

Build/verification source:
`/home/khing/.cache/parasaur-enterprise-current.c9s77dxz/source`.
Build and Auth logs are in its parent directory. These were two logical devices
and separate workspace accounts on local fixtures, not two physical machines
or a real provider subscription/bill.

## Still unfinished

The private source and local build are not deployed. The public recovery ZIP and
installed WSL user CLI remain at `16e9ecc6` and exclude this private consumer.
No native desktop source or installation changed. Existing worker releases and
jobs were left alone.

Compute-side job consumption, portable runner/model/skill aliases, subscription
owner and execution identity/ledger integration, filesystem/environment/credential
and subprocess isolation, staged configuration migration, hosted two-machine
acceptance, signed updates and physical iPhone acceptance remain open. The
inference fixture does not establish that a real cloud factory job completes.
