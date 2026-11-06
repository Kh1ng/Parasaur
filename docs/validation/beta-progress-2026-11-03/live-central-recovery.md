# Live central recovery — 2026-11-05

## Deployed runtime

Parasaur is available only over the tailnet at
`https://hermesagent.tail82695.ts.net:8443/`. Hermes retains port 443 and its
existing proxy target. Parasaur's API remains bound to `127.0.0.1:3773`.

The server/web runtime is public core
`6b101b983ed7db6ac6f96a0de323fc0fa255d656`, version 0.3.0. The original
`parasaur-central-6b101b98.zip` is immutable and still contains its original
CLI. Subsequent CLI repairs are separately identified below.

The deployment retained the service user, working directory, config,
coordinator credential, node identity and all 95 chat sessions. The VM checkout
HEAD and tracked source were unchanged. Authenticated tailnet `/api/info`,
health, main JS/CSS and byte-for-byte reads of all 29 JavaScript chunks passed.
The deployment report's `verificationSeconds` includes asset checks and is not
an isolated startup-time measurement.

The original asset failures were root-owned JS/CSS files with mode 0640.
Service-user readability was repaired. Core `fc591a40` makes future deployment
publish assets with `a+rX`, then atomically replace an index installed as 0644.
The restrictive-umask regression failed on the previous implementation and
passes on the fix; all 91 updater cases pass. Operator files outside the
published assets retain their permissions.

Rollback is retained at
`/home/khing/.local/state/gah/central-repair-20261105/runtime-rollback/`.
Deployment records and later memory evidence live in its parent directory.

## Memory investigation

The controller's existing 1.5 GiB cgroup limit was repeatedly exhausted. The
ledger contained 26,327 rows / 248,764,099 bytes; the event journal contained
roughly 323,113 rows / 209 MB. No execution history was deleted or truncated.

Core `0a713eb8` makes work-ID alias indexes borrow canonical entries instead
of cloning records for each alias. The canonical and legacy aliases share the
same underlying entry, verified by a pointer-identity regression with a large
payload. Under a 1100 MiB transient cgroup, the real `status --light` command
completed with maximum RSS 317,744 KiB. All snapshot fields matched the prior
candidate except generation time and skill-inventory freshness. These are
bounded, sequential observations, not an unconstrained heap/speed benchmark.

The first fixed CLI (SHA-256
`0fb2046597637a843849dc60c5f1183465f84aff834dd8f2fed6a7b24af1b957`)
still exhausted the controller limit on a longer run. A temporary tracing build
identified overlapping full-history reads in blocker reporting, review
lookups, telemetry export and terminal-failure notification deduplication.
Tracing recorded callers, row counts and RSS only. Its temporary service
override was removed and the normal ExecStart restored after the experiment.

Core `2da5846c5dfd156877826b88a0b52d3978de418d` reuses the pass's event
snapshot for blocker reporting, streams notification deduplication and
telemetry export, filters previously exported telemetry before accumulation,
and retains only relevant review/work-item history. Review reset tombstones,
legacy aliases, append order and existing repository attribution are preserved.
Every ledger record is still validated; corrupt discarded records fail the
scan. Telemetry writes occur only after complete validation and release of
the ledger lock.

The optimized CLI SHA-256 is
`cb2cd32c0f414fc43daa4b39f7e72056c151719e969b85e49c28e517d74e8698`.
593 source files were compared against the committed Git archive. Trace hooks
are absent. Formatting, all-target/all-feature Clippy with warnings denied,
121 ledger, 552 dispatch, 40 status, 46 sync, 198 controller, 43 telemetry,
43 notification, 91 updater and 3 review-head cases passed. New checks cover
filtered-history corruption, bulk/streamed export deduplication, watermark
ordering and absence of partial export on a corrupt tail. The optimized
128 MiB event-journal check passed within a 192 MiB address-space limit.

This CLI was installed atomically without restarting the API. The immediately
previous CLI is retained as `gah-before-streamed-history`. The live trial failed:
it ran for 753 seconds with the same PID and no restarts, then the monitor
stopped it near the unchanged cgroup limit before another OOM. Its result and
samples are `streamed-history-controller-result.json` and
`streamed-history-controller-samples.json` on the central VM and in the local
ignored artifact directory. This is evidence against calling the leak fixed.

Core `1da5043d` separately fixes a live pipeline stall: a typed review-budget
refusal was being returned as a worker infrastructure error, causing a
five-minute controller-wide backoff. The controller now reports a blocked review,
keeps that item excluded from immediate follow-up and continues unrelated work.
Direct dispatch retains its typed error and terminal event. Budget events now
carry `retry_budget_exhausted`. The regression checks refill eligibility,
non-admission of the hold and preservation of actual error classification;
199 controller cases, 552 dispatch cases and all-target Clippy pass.

Core `108c9f592561014b5d94b97d83a8ccf03ee7b5d1` additionally validates
routing history without retaining empty review refusals. Actual calls, verdicts,
approval state and post-review repair correlations retain their prior semantics.
The differential routing regression passes. All 199 controller and 553 dispatch
cases pass, as do all-target/all-feature Clippy and the optimized build. One
parallel build-cache fixture failed with a missing availability temp directory;
the focused repeat and full serial dispatch suite passed. Its failure is retained
in the validation log rather than counted as a clean initial run.

This optimized CLI's SHA-256 is
`ec60c6b835ae033bdd9798b54c92828d33aa03d5cb0a98b05c393af30d7ad2b6`.
Its 593 source files match the committed Git archive; trace hooks are absent.
The second finite live trial completed 904 seconds with the same PID, no
restarts and the original memory limit. Cgroup peak was 1,044,803,584 bytes;
last sampled memory was 675,135,488 bytes. The controller was left running.
Records are `routing-history-controller-result.json` and
`routing-history-controller-samples.json` on the VM and in local ignored artifacts.
This proves a short live orchestration trial, not multi-hour stability or memory
behavior with active agent backends. Actual review launches remain capacity-deferred.
The final trial also encountered a Git-fetch timeout and infrastructure backoff;
the typed budget-refusal change does not suppress transport failures.

The WSL user CLI now matches this binary, with rollback at
`~/.cache/parasaur-upgrade-backups/gah-before-108c9f5`. Separate worker release
binaries and native desktop bytes were not changed by that user CLI replacement.
Private assembly `396faf0` pins core `108c9f5` and passes its combined server
suite (1,101 passes, 38 explicit skips), 16 mock cases, production build,
packaging/input guards and transport tests. This remains local assembly evidence.

## Client and remaining acceptance

The current Windows native client (source `018f6603`) loads authenticated live
Overview and Factory pages from the tailnet origin. Owner access was verified
and saved in Windows Credential Manager using the native client's existing
keyring format. It is left open; no credential value is included in the evidence.
The older separately installed GAH Worker 0.1.1 client failed remote access and
was closed after the check; it was not used as proof of the current client.

The live UI exposes additional acceptance work: the old Windows worker's claim
observation is unreachable and still advertises the old central origin, deferred
capacity outcomes appear as failed activity, the header's build revision is
unknown, and navigation overflows at the observed desktop window width. These
are unresolved findings, not passing UI-polish acceptance.

The MacBook's installed native client is open. Its HTTPS owner credential was
saved in the login Keychain and verified against the live API. Temporary
central testing/build cleanup from the earlier Mac pass remains complete.
The current Linux controller changes do not constitute a new Mac release or
prove desktop WebKit memory behavior under active use.

Live factory progression, real provider/account/usage/limit reconciliation,
Windows worker upgrades, the workspace/Git workflows and distribution signing
still require acceptance. The central's worker reservation plus adaptive floor
exceeds its available host memory; capacity deferrals must remain visible and
must not be bypassed to make a trial pass. Mobile is deferred from this pass.


## Extended observation and shared web follow-up (2026-11-05)

The extended observer did not pass. It stopped the controller after 1,371
seconds at its conservative memory threshold, with the same PID and zero
restarts. The last sample had 1,523,449,856 bytes of cgroup memory: 728,383,488
anonymous bytes and 782,577,664 file-cache bytes. RSS was 710,128 KiB. The
recorded cgroup peak reached the original 1.5 GiB cap. The controller was left
stopped; the API was not stopped. This is neither proof of an anonymous-memory
leak nor evidence of multi-hour stability. Issues #1681 and #1680 remain open;
the Mac client has not received an equivalent memory attribution check.
The VM's `extended-controller-result.json` retains the failure report.

The shared web assets at source `d0722750` were subsequently deployed with
atomic index replacement and retained old chunks. All 30 asset files (29
JavaScript chunks and CSS) matched the local manifest byte-for-byte over the
tailnet HTTPS origin. The server-injected bootstrap, API PID, and CLI hash were
preserved. This provides HTTP/deployment evidence, not native visual acceptance
of the new deferred-activity labels. The Windows worker and protected launcher
recovery are documented separately in `windows-worker-recovery.md`.
Private assembly `875f7d6` pins core `d0722750`; 1,106 server tests passed with
38 explicit skips, plus 16 mock cases, production build/typechecks, packaging
input guards and transport tests. The enterprise build has not been deployed
as a live GitLab SSO service.


## Expired quota display repair (2026-11-05 local / 2026-11-06 UTC)

Shared web source `c648ce94` is deployed. Passed or invalid reset dates no
longer select the current quota limit, fill a current usage ring, or produce a
reset countdown. Their original percentages and reset dates remain explicitly
last recorded. Valid sibling windows retain their own percentages; no new
allowance or zero is inferred. The navbar clock advances without requiring a
hover or new API snapshot. Recorded ledger costs retain their independent
actual/estimated/unknown labels. The Kanban usage summary uses the same rule.

Validation: 18 focused unit tests, three component tests, three subscription
end-to-end tests, web typecheck and production build passed. The component
regressions cover the reported expired Mistral monthly window and expiry of a
mounted navbar. Existing account, pool, recorded-cost, and live-activity checks
remain covered. The deployed HTML retains its server bootstrap and all 30
asset files match the manifest; API PID and CLI bytes are unchanged. Native
visual acceptance was not performed. Evidence is in the local ignored
`artifacts/parasaur-web-c648ce94/` manifest and the VM's
`expired-quota-web-deployment.json`; deployment completed at
2026-11-06T04:24:45Z. Retained old chunks and a backed-up index permit rollback.

This repairs the expired-window presentation, not full usage reconciliation.
Provider refresh/identity/pool correctness still requires live acceptance, and
persistent client authentication plus the deployed GitLab SSO path remain
unfinished. The two new user reports have not been filed or counted as closed.


## Streamed recovery readers (2026-11-06 UTC)

Source `befb656a` replaces the controller's full `read_events` snapshots with
streamed queries. Abandoned-run reconciliation retains only lifecycle IDs;
capacity suppression and repeated human reports retain the latest matching
record per work item; stuck-loop detection folds the selected action's two
suffix counts. The complete journal is still validated before reconciliation
writes, so corruption cannot silently produce partial recovery. Ledger/event
files remain intact. The selected-action query adds a journal scan instead of
keeping a historical event vector resident during selection.

Validation: 200 controller tests, 12 event tests, a differential reader check
covering 64 histories, resets, generations and legacy/out-of-order boundaries,
all-target/all-feature Clippy with warnings denied, formatting and optimized
build passed. The source manifest verifies 586 Rust/Cargo files against the
commit with only CRLF/LF normalization. CLI SHA-256 is
`8907144d7e394192be7f77b35d0920176ff3b4333b0e6d2e71af9971bf846f90`.
It is installed on the VM, with the previous CLI backed up at
`~/.local/state/gah/central-repair-20261105/gah-before-streamed-recovery`.
The API PID and deployment were preserved.

The controller was restarted under its original 1,572,864,000-byte limit.
A one-hour observation is running in the separate transient user unit
`parasaur-streamed-recovery-observer-20261106.service`, with a conservative
1,450 MiB stop guard. Its metadata-only samples/result use the prefix
`streamed-recovery-controller` in the existing VM evidence directory. The
trial is not yet a pass; inspect its live unit and result before claiming
completion or restarting it. An early five-minute sample had the same PID,
zero restarts and zero cgroup OOM events. This is not long-run acceptance.
