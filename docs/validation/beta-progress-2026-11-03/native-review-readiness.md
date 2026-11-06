# Native reviewer readiness and routine/escalation overlap

Windows now has central's effective factory routing, review/paid-cycle caps,
publishing and validation policy. The native configuration model checked the
transfer while preserving node-local paths, credentials, CLI arguments and
runtime settings. Unbound `agy-second` and `cli-router` entries are disabled
placeholders; they cannot silently use ambient credentials. No second factory
loop was started. Backups remain in the worker's private recovery directory.

The packaged `gah-reviewer` and `gah-implementer` OpenCode agents were missing.
Both were installed from the canonical tracked blobs, and OpenCode's own agent
catalog confirms both names. A native dry-run selects the configured routine
reviewer `opencode/tak-mistral-vibe/zai-glm-5-3` without provider calls.

## Native review proof

An earlier connectivity-only prompt lacked a review pack. The reviewer correctly
refused to claim review readiness. The replacement diagnostic supplies a project
brief, numbered source criteria, an actual diff and local test results for a
small isolated Python function. Hosted CI is explicitly `not_run`.

The real native `run_review_backend` invokes the installed role agent and the
configured model. For `return a - b`, local addition tests fail; for `return a + b`,
they pass. Both provider processes complete without agent fallback or changes
to fixture files. Usage artifacts are written. Nothing is pushed or published.

The raw fixture replies also pass the production review parser, not a diagnostic
copy of its rules: the broken case is `NEEDS_FIX` with one grounded actionable
finding, and the repaired case is `APPROVE` without a human handoff. The parser
runs in an isolated source checkout with a temporary test added beside its
private implementation; the repository's public API is unchanged.

## Overlap defect and correction

The actual policy declares the exact Mistral route as routine, plus an OpenCode
escalation entry with no model restriction. Escalation matching ran first and
therefore classified that ordinary routine route as `escalatory`. This matters
beyond its label: an untried escalatory reviewer receives an exemption from the
normal review-cycle cap, so overlap could grant an extra routine call.

Public `24d9aa2b` gives an explicitly configured routine backend/model precedence
over overlapping escalation entries. Other escalation models retain their tier,
and an unspecified routine model still permits a specifically configured
escalation. Approval evidence, paid caps, capacity admission and append-only
history are unchanged. The fix tightens the existing cycle cap.

All three new regression tests fail before the correction and pass after it.
All 84 review tests pass, as do formatting, Clippy with all targets/features and
warnings denied, and the optimized release build. The same live provider replies
pass the production parser under the corrected `strong` routine classification.
The retained binary was compared against 587 tracked Rust/Cargo inputs; only
the isolated test checkout contains the additional diagnostic test.
Private assembly `c45a789`, pinned to `24d9aa2b`, also passes 1,119 server tests
with 38 explicit skips, 16 mock tests, production builds and private input,
artifact and credential-transport guards. Its log is
`/home/khing/.cache/parasaur-enterprise-current.n5hss1nf/build.log`.

The Windows user CLI and active worker CLI are updated to the verified binary,
SHA-256 `a255cf50bc8f221ca026e8b2f5c021aba886feda78effc82c8761f07415243a7`.
The first deployment check expected spaces in the dry-run display and rolled
back both binaries. The corrected exact-format check then passed. Worker PID
178252, configuration, identity and credentials are preserved; no worker
service restart or controller start was needed.

Central's CLI is updated to the same binary. The current-PID memory observation
finished before the restart. A fresh check found no active work across 100 chat
sessions, then deployment waited for an idle controller child-process boundary
and confirmed no active central lease. Controller PID changed from 604933 to
638807. API PID 628728, credentials, configuration, MemoryMax 1,572,864,000 bytes
and MemorySwapMax zero are preserved. A checked backup is retained for rollback.

## Remaining execution evidence

At 09:59 UTC central is healthy, but #1525 still reports
`node_capacity_deferred`. Its controller chooses local execution; Node's fleet
coordinator handles separately submitted sessions. Provisioning a capable worker
does not connect those paths or preserve central attempt history automatically.
Starting another loop with a different ledger is not full pipeline acceptance.

After deployment, the fresh event read at 10:19 UTC shows a changed gate:
#1525 reached `review_budget_exhausted` and then `human_required`, reason
`retry_budget_exhausted`. The first cold last-decision read exceeded 20 seconds;
the retry returned successfully while health remained healthy. This unresolved
cold-read latency is separate from controller liveness.

A streaming read of the source ledger found five budget-eligible records for
the current review generation, matching profile and repository. Dated October
30 through November 2, each has effective backend `auto`, no selected model,
null validation result, zero recorded attempts and `no_attempt_started` usage.
The budget filter currently counts these records. Whether they prove no backend
invocation needs a writer-lifecycle audit before changing accounting; missing
usage alone is insufficient. No attempts, review budgets, approvals or old
records were reset or rewritten. This is the next concrete factory investigation.

The controller cgroup's near-limit total also includes substantial inactive file
cache. A separate current-PID observation distinguishes resident/anonymous memory
from cgroup file cache: PID 604933 completed 600 seconds/41 samples without an OOM
kill. RSS ranged from 535,736 to 650,576 KiB and ended below its initial reading.
This bounded observation does not establish multi-hour memory acceptance.

A ten-hour observation is now running against PID 638807, with one metadata-only
sample per minute. The observer is a separate user service
`gah-beta-memory-638807.service` (observer PID 639062), capped at 64 MiB and 36,600
seconds. Its report is the private repair file
`controller-memory-638807-soak.json`. It terminates if the controller changes;
an interrupted/failed observation is not a pass. The running observer and its
initial sample were verified. Do not restart either controller or observer
merely because an observation call times out. The Mac memory report #1680 and
GitLab proxy management-access/real signed-in persistence tests remain open.

Metadata-only evidence is in ignored `artifacts/central-live-20261105/`:
`worker-factory-policy.json`, `worker-review-agents*.json`,
`worker-native-review-fixtures.json`, `worker-native-review-production-gate.json`,
`worker-review-tier-cli-deployment.json`, `native-review-factory-boundary.json`
`central-review-tier-cli-deployment.json`, `current-controller-memory.json`
`factory-boundary-after-review-tier.json`, `review-budget-receipts-1525.json`
and `controller-memory-attribution.json`. Fixture sources and owned diagnostic
outputs are retained under the worker recovery directory. No real issue, PR,
assignment or production ledger was changed by the fixture review.
