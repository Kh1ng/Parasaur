# Compute process boundary — 2026-11-04

Verified private revision `bbbd95996cb03520232ae305d6bc159b402a2a1e` against
public core `16e9ecc6f5ca8bd5a0db16c64574d1b6bde8dc17`. This implements and
tests the process boundary for compute execution. The cloud compute queue
consumer is still absent; node startup does not enable this boundary or execute
cloud jobs. It is not complete shared-hardware or enterprise acceptance.

## Change

The existing runner's environment filtering does not isolate host files or
credentials. The new private `runIsolatedCommand` requires an explicit prepared
Linux image, private workspace and exact execution identity. It uses actual
Bubblewrap namespaces and a user systemd scope; unsupported configurations fail
without falling back to host execution. No ambient home, root filesystem,
network or environment is mounted. The image is read-only; home and temporary
storage are bounded memory filesystems. The scope establishes memory, swap,
task-count and runtime ceilings before the program can execute.

An authorization gate runs after kernel setup but before the program starts.
Denied or late approval cannot release a cancelled program. An early stop can
race scope creation, so cleanup kills the launcher and stops/checks the scope
again after launcher exit. Detached descendants are included. Unknown cleanup
rejects with the stable scope ID rather than permitting a caller to release its
capacity reservation blindly. stdout/stderr retain bounded diagnostic tails.

The complete repository workflow must eventually run inside the boundary,
including Git hooks and validation. Wrapping just the agent would leave host
execution paths open.

## Verified

- Fresh official combined build: 1,017 server passes, 11 skips, zero failures;
  13 mock passes; server/web typechecking and production builds; private artifact
  and build-input guards; transformed transport checks.
- Dedicated `npm run test:isolation`: nine actual Linux sandbox checks passed.
  Only the native unsupported-platform check skipped. The normal build's eleven
  skips comprise these ten isolation-file cases plus the earlier opt-in check.
- Host file/symlink escape attempts fail; fake ambient credentials are absent;
  host processes and loopback listener are unavailable; runtime writes, mounts
  and nested user namespaces are denied. Explicit job environment is retained.
- Actual cgroup `memory.max`, `memory.swap.max` and `pids.max` match the request.
  Excess forks fail and excess allocation terminates the bounded job.
- Denial, approval arriving after cancellation, cancellation during scope
  creation or execution, deadline expiry and abrupt Node controller death do
  not leave executing descendants. No test scopes remained after verification.
- Synthetic large stdout/stderr remain bounded. A missing tool or a non-sandbox
  executable cannot authorize/run the payload through a fallback.
- Native Windows: the unsupported-platform refusal passed; nine Linux checks
  explicitly skipped. Native Windows execution is not supported by this module.
- Exact-revision SQL migration replay and real Auth/PostgREST inference,
  concurrent receipt retry, revocation, HTTP/WebSocket relay, identity,
  entitlement and removal checks passed again. Those provider calls use a local
  fixture HTTP server, not an actual paid subscription.

The tracked test preparer downloaded and extracted Ubuntu packages into a
unique user cache directory, without package installation scripts, global
installation or administrator approval. Its Python-only runtime was then used
by the dedicated check against the official assembled build. It contains no
factory/provider tools and is not a production runtime.

The verification checkout remained clean at
`/home/khing/.cache/parasaur-enterprise-current.8kjlekhi/source`; its sibling
`build.log`, `isolation-verification.log` and `auth-verification.log` retain the
results. Its release manifest names the full revisions above.

## Remaining

Implement the compute queue consumer and durable local reservation recovery;
build the dispatch/provider runtime; provide narrowly scoped inference/forge
access; resolve portable workspace aliases and record their identity/skills in
the ledger; integrate shared local/cloud capacity. Production workspace storage
also needs a bounded disk allocation strategy: this primitive currently binds
an explicit writable workspace and does not impose a disk quota on it.

Staged configuration migration, hosted auth/deployment, real remote nodes and
provider-backed execution remain required. The public recovery ZIP, installed
desktop/CLI and live host were not changed by this private implementation. Mac
execution remains deferred. The full beta checklist stays open.
