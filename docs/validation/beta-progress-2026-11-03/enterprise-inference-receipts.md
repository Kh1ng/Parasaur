# Enterprise inference retry receipts, 2026-11-04

Private runtime `14c160689e43434284b5d0d335ff374d21f10967` now pairs with
public core `16e9ecc6f5ca8bd5a0db16c64574d1b6bde8dc17`. The assembler retains
its reviewed overlay base and merges the newer core without conflicts. This
includes the public exact factory node/account/model work while preserving the
private authentication, relay cancellation and shutdown repairs.

## Reproduced defect and repair

Tracing TICKET-245 found that the cloud execution SQL has queue and permission
contracts but no queue-consuming worker implementation. Before adding that
consumer, a regression exposed an inference retry defect: polling a provider
response deleted its request row. Enqueueing the same request key afterward
created another request ID eligible for another provider call. A second poll
also failed instead of returning the delivered response.

Both regressions failed against the old functions. The additive private
migration now keeps the accepted identity for the live job, returns the same
receipt for identical retries, rejects changed arguments and permits repeated
response reads until the existing deadline. Expiration refuses replay rather
than enqueueing another call. Expired payloads can be cleared while their
fingerprints remain. Terminal job transitions clear their inference records,
including a revocation that cancels work on renewal.

New admission bounds receipt count and retained/reserved response capacity.
Accepted retries bypass new-admission capacity checks; capacity refusals never
select a sibling subscription. Tests exercise reservation of future response
bytes as well as stored response bytes. This is queue/storage evidence, not a
measurement of provider billing or Mac process memory.

The rollout guide remains in the private checkout. Previously deleted request
identities cannot be reconstructed: work started before the migration cutover
requires explicit reconciliation. The persisted cutover does not move on replay.
The change is not deployed and does not stop or rewrite existing local jobs.

## Final-source verification

The clean private commit above passed a fresh assembled production build:
**1,005 server tests passed, one opt-in skip, zero failures**, plus **13 mock
tests**, server/web builds and typechecks, private artifact/input guards and
transformed cloud-transport checks. The release manifest records both exact
revisions. The final private checkout remained clean.

All disposable SQL migrations replayed twice. The full authorization suite
passed, including existing member, device, compute/inference grant, revocation,
dashboard and queue checks. Added coverage verifies delivered/expired retries,
argument conflicts, full-queue retries, receipt and response-capacity limits,
cutover refusal, payload cleanup and absence of another provider claim.

The real disposable Auth/JWT, edge, PostgREST and HTTP/WebSocket relay fixture
also passed against that final source. Eight concurrent inference retries
returned the same receipt through PostgREST, and no second provider request was
claimable. Completion cleared private inference records and refused further
response reads. Fixture enrollment preserves the earlier health-revocation test
by creating a fresh execution device. Anonymous device-RPC denials are checked
using PostgREST's actual 401 mapping.

Build/verification source:
`/home/khing/.cache/parasaur-enterprise-current.n0qdbah9/source`.
Build and Auth logs are in its parent directory. Fixture services were stopped.
No hosted migration, membership, entitlement, provider account or job was changed.

## Remaining scope

The public recovery ZIP and installed user CLI remain at `16e9ecc6`; private SQL
and enterprise artifacts are separate and are not included in that ZIP. No
native desktop change or reinstall was needed for this private work.

Portable aliases, an actual cloud queue consumer, shared-hardware credential /
filesystem / environment / subprocess isolation, and staged local/cloud config
migration remain implementation work. A WSL user/mount/network/PID namespace
probe succeeded; that does not prove isolation or enable shared native execution.
The earlier two-account browser evidence remains recorded at its own revision;
this pass verified APIs and the combined build rather than repeating that UI
workflow. Hosted, real-provider, signed-update and physical-iPhone acceptance
remain open. The overall finishing goal is not complete.
