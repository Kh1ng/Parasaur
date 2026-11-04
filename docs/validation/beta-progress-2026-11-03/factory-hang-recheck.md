# Factory hang recheck — Windows/WSL, 2026-11-04

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
