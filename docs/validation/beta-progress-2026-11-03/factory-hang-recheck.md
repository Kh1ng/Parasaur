# Factory hang recheck — Windows/WSL, 2026-11-04

## Live admission investigation and shared-web correction — 2026-11-06

The live controller is still PID 437234, with zero restarts. Its latest 100
activity receipts in the one-hour endpoint window were node-capacity deferrals;
this endpoint is capped at 100, so this is not a total count for the hour.
The latest receipt for #1525 recorded 2,509 MiB available against a 1,024 MiB
review reservation and 2,048 MiB safety floor. Current journal entries reached
the prepared review diff before admission. This establishes the present wait;
it does not resolve the earlier intermittent 300-second Git-fetch timeout.
No provider backend was forced to start and no capacity guard was lowered.
The status API's work blockers list did not contain this admission wait.

Shared web `c0668cae` now derives the explanation from the controller receipt
already served by the API. Kanban keeps the latest deferred review/fix in
Waiting and identifies whether the last attempt waited for node or provider
capacity. Its why-not detail retains the refusal, timestamp and any prior
backend-attempt evidence, removes the route-state fingerprint, and offers the
relevant resource/account check. It does not claim that old resource readings
are current. Runtime worker evidence, claims, human gates, dependencies and
closed/merged work retain precedence. A later terminal success or new running
action supersedes the waiting summary. Overview wraps its last-decision
receipt instead of clipping the admission reason behind a tooltip.

Validation: 17 focused unit tests, 20 component tests, 12 distinct Overview and
Kanban end-to-end tests, typecheck and production build pass. The component
check verifies un-clipped refusal text at 960px and 360px; no computer control
or native visual acceptance was performed. One old component assertion
expected deferred work in In review; it now explicitly expects Waiting.
The browser integration checks the real app's controller-activity read,
receipt-to-card derivation and numeric admission detail using fixture services.

Deployment completed at 2026-11-06T06:19:41Z. All 30 served assets match the
immutable archive manifest. API PID 302122, HTML bootstrap and installed CLI
bytes were preserved; the controller was not restarted. Evidence is in ignored
`artifacts/central-live-20261105/live-capacity-receipts.json` and
`factory-capacity-receipts-web-deployment.json`. This makes the live wait
inspectable; extra execution capacity, remote factory execution and full
claim-to-publication acceptance remain open.

Private assembly `69524ff`, pinned to `c0668cae`, passes 1,106 server tests
(38 explicit skips), 16 mock cases, production builds/typecheck and private
artifact/input/transport checks. Its isolated WSL log is
`/home/khing/.cache/parasaur-enterprise-current.2eofbwne/build.log`.

One related server-side gap remains: `activityFromController` currently turns
ordinary capacity-refusal terminal events into `dispatch_failed` notifications.
The controller history still records the full refusal, and the shared UI
qualifies it as deferred, but the notification conversion needs its own fix.
Do not claim that this web deployment corrects existing notification history.

## Earlier fixture and hosted investigation

At finishing-branch revision `2873321c`, the current cached debug CLI and
`controller_refill_regressions` executable completed all 21 tests under this
Windows machine's Ubuntu-24.04 distro. The Rust sources were unchanged by the
subsequent web commits. Tests used the existing isolated fake workers, forge,
gateway setting and persistent Linux target directory; no live worker started.

The target ran with `--test-threads=1 --nocapture`, an explicit
`CARGO_BIN_EXE_gah=/home/khing/.cache/parasaur-beta-target/debug/gah`, Linux PATH
and `CARGO_TARGET_DIR=/home/khing/.cache/parasaur-beta-target`. An external
monotonic timer timestamped each completed test. Result: **21 passed in 114.28
seconds**. The three paths named in #1662 completed:

| Test suffix | Elapsed for that serial test |
| --- | ---: |
| `refills_with_missing_availability_after_a_fast_completion` | 8.42 s |
| `reprobes_node_pressure_with_active_worker_remaining` | 4.66 s |
| `reviews_a_finished_ticket_while_a_slow_sibling_still_runs` | 8.43 s |

The quota-scaling/boost/weaker-reading scenario took 21.64 seconds; the
node-pressure/scaled-limit scenario took 11.59 seconds. These local timings
identify current costs, not the cause of an intermittent hosted hang.

## Hosted evidence

The current [issue #1662 discussion](https://github.com/Kh1ng/Parasaur/issues/1662)
reports that #1675 fixed the fake-worker active-count race and removed host
memory-gateway interference. Its remaining diagnosis is a genuine hang that
was not reproduced locally, including prior CPU-load attempts. This differs
from the issue's initial runner-load hypothesis; do not assume shortening a
tick interval repairs the remaining failure.

For commit `84b8f3f6aa5531b36628d0bc78adcb960e1a29d9`, the workflow API reports
the latest CI attempt as successful. Reading the first attempt separately
found the failed [Rust job 121610412598](https://github.com/Kh1ng/Parasaur/actions/runs/40333954983/job/121610412598).
The job log confirms the refill fixture timed out at 120 seconds, with both
workers' final printed stage “Attempt 1/3: running codex backend...”. The target
finished with 17 passed/1 failed in 183.35 seconds. That revision predates the
process-tree snapshot from #1675, so this log cannot identify the blocked child,
wait channel or lock. The later successful rerun is not proof that the original
failure was resolved.

No scheduler, deadline, assertion or fixture behavior was changed from these
observations. A failure with the current process-tree diagnostic is the next
decisive evidence. The local run does not meet the under-60-second,
three-consecutive-hosted-runs criterion. The issue remains open; no rerun,
comment, claim, push, live service or remote machine was changed.
