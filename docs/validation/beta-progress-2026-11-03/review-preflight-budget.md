# Review budgets distinguish preflight failures from review calls

The live #1525 hold counted five current-generation review records dated October
30 through November 2. All match the configured profile/repository and head.
They record schema 11, backend `auto`, no selected model, zero started/completed
attempts, no attempt or route records, no exit result/verdict, null validation
result, and the explicit `no_attempt_started` marker. Failure attribution is
`harness_error` / `dispatch`.

The writer audit establishes their boundary. Dispatch appends its terminal row
after the review workflow returns. Review increments `attempts_started` and
applies its route before entering the invocation path. Completed invocations
also record attempts, completion/exit evidence and available usage. Controller
crash reconciliation uses `abandoned` records, not invented zero-attempt reviews.
These preflight rows were nonetheless counted by the review-budget filter.

## Correction

Public `742512b7` excludes only proven unstarted reviews: explicit zero counters,
no attempt/route record, no exit result, no validation/verdict, the persisted
`no_attempt_started` marker, and no usage observation contradicting that marker.
Missing counters/markers, legacy unknown telemetry and conflicting execution
evidence remain charged. Even a reported zero token/cost observation retains
its charge. Actual review calls, same-head scope, repository/profile/work-item
scope, paid caps and configured limits are preserved.

The status projection must also reconsider the old refusal, or the corrected
dispatch count would never be reached. It now shares the canonical receipt
filter and can release only a matching recorded `review_budget_exhausted` hold
when both configured caps have room. It does not release other retry holds,
owner/policy approvals, missing-generation evidence or genuinely spent budgets.
Zero limits remain disabling. This is a projection over append-only history;
no record is deleted, rewritten or reset, and no contract-version bump discards
old real review calls.

## Verification

- The new preflight-count regression fails on the prior implementation and
  passes after correction. Contradictory/missing-evidence cases stay charged.
- All 86 review tests, 13 status-gate tests and 200 controller tests pass.
- Formatting, Clippy with all targets/features and warnings denied, and the
  optimized release build pass. The only initial Clippy failure was a test
  function-pointer tuple type, corrected with a named mutation type.
- The binary matches 589 tracked Rust/Cargo inputs. SHA-256 is
  `b211cdd60bff1ee02883c0b6901ca0cf1244a74195ceae0641bd7ab0d361a8c0`.
- A real read-only status comparison used the unchanged central configuration
  and ledger. Both binaries saw PR #1678, head
  `d46ae6e1605de4ade7c8a5095dc03bb5f5ef61a4`, the same review generation and
  classification `NEEDS_FIX`, with no observation errors. The installed prior
  binary projects `retry_budget_exhausted`; the staged binary projects no such
  blocker. No provider prompt or publication was made by this comparison.
- Private assembly `b2e4879`, pinned to `742512b7`, passes 1,119 server cases
  with 38 explicit skips, 16 mock cases, builds and private input/artifact/
  credential-transport guards. Log:
  `/home/khing/.cache/parasaur-enterprise-current.c92pxev9/build.log`.

## Deployment and remaining gate

The corrected CLI is installed on Windows/WSL and central. Windows worker PID
178252, its configuration/identity/credentials, and exact dry-run Mistral route
are preserved; its service was not restarted and no worker loop was started.

A fresh central preflight checked 100 sessions with no active chats/jobs, then
waited for an idle controller boundary and checked for active leases. Central's
controller changed from PID 638807 to 666974. API PID 628728, configuration,
credentials, MemoryMax 1,572,864,000 bytes and MemorySwapMax zero are preserved.
Checked backups remain in each node's private repair directory.

The installed binary repeats the live status result: same PR/head/generation,
no observation errors, and no review-budget blocker. Subsequent controller
events reach `context_built` and then `node_capacity_deferred` at 10:53 and
10:54 UTC. The false accounting hold is resolved; no actual review, repair,
validation or publication is claimed from those capacity-refused dispatches.

## Memory observation correction

The first central rollout attempt made no change: its guard found the previous
sampler had failed, while controller PID 638807 remained active with zero
restarts. That sampler collected 17 samples through 961.46 seconds, then its
five-second `systemctl` query raised `TimeoutExpired`. This is an incomplete
observation, not an OOM or a ten-hour pass.

The corrected sampler binds PID, process start ticks and cgroup. Periodic samples
read `/proc`/cgroup directly instead of issuing `systemctl` queries. Missing
measurements are recorded as gaps; completed-with-gaps and changed-process
outcomes remain distinguishable from a complete run. A direct identity check
passed before use.

### Completed ten-hour observation, with a cadence limitation

The existing observer finished at 20:52:43 UTC on November 6; its unit was
collected normally with result success and exit status zero. Controller PID
666974 retains process start ticks 117939675 and zero service restarts. Its
597 samples span 36,000.003 seconds, with no cgroup OOM kills. The service's
1,572,864,000-byte memory cap and zero swap limit are unchanged.

The report says `complete` and has an empty `observationGaps` list, but that
list records read failures rather than delayed samples. Direct cadence analysis
finds one 344.081-second interval (between elapsed 7,500.175 and 7,844.256)
against the configured 60 seconds; all other intervals are under 61 seconds.
Consequently this is a completed-duration observation, not uninterrupted
minute-by-minute coverage. The delayed interval's cause is not established.
Do not infer a sampling pass from the report's status field alone.

Peak observed process RSS is 835,952 KiB. Peak cgroup charge reaches the
configured cap, including reclaimable file cache; process RSS, anonymous memory
and total cgroup charge are different measurements. A subsequent read shows
1,523,743 `memory.events:max` occurrences and zero `oom`/`oom_kill` events;
no initial max-counter sample exists, so that total cannot be attributed wholly
to this observation. The limit remains relevant despite absence of an OOM kill.

Factory review attempts remained capacity-deferred. This observation does not
prove sustained agent-load behavior, a leak fix, or Mac client acceptance and
does not close #1681/#1680. Neither a replacement sampler nor a controller
restart was started. The full numeric observation is retained locally at
`artifacts/central-live-20261105/controller-memory-666974-completed.json`.

At its initial checkpoint, the ten-hour observation was active as `gah-beta-memory-666974.service`,
observer PID 667504, bound to controller PID 666974. It retains the 64 MiB
observer cap and 36,600-second lifetime. Three initial samples and no gaps were
verified at 10:54 UTC. The report is the private repair file
`controller-memory-666974-soak-v2.json`. Check that existing unit/process and
report before deciding whether it is running or terminal; observation timeouts
must not trigger a duplicate monitor or controller restart.

The artifact is `artifacts/parasaur-review-budget-742512b7/`. Metadata-only live
records are in ignored `artifacts/central-live-20261105/`:
`review-budget-receipts-1525.json`, `review-budget-baseline-1525.json` and
`review-budget-candidate-1525.json`, plus
`review-budget-installed-1525.json`, `central-review-budget-cli-deployment.json`,
`worker-review-budget-cli-deployment.json` and `factory-after-budget-fix.json`.
The read-only comparison proves corrected
accounting/projection for this history, not successful repair, validation,
publication or full distributed factory progression.
