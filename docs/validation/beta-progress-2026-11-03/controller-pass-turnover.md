# Bounded controller passes — 2026-11-05

Public core `ded751a21e31c3aa4ecdd33fe4377f7b0d3bb41e` and private pin
`c7b2a06b5711750ed93044952faf5a8116909b08` establish a lifecycle boundary for
parallel passes under sustained queue load. This follows the
[event-reader repair](event-reader-memory.md) and [capture repair](rust-output-memory.md).
It is not live attribution or closure of Linux memory issue #1681.

## Reproduced behavior and repair

Every successful worker reopened the fill budget. While new eligible work
continued arriving, a parallel pass could therefore run indefinitely. Its
completed results and work exclusions remained alive until the pass ended.
The recurring loop's config reload, periodic work and pacing also waited for
that boundary. The existing worker concurrency limit did not limit the total
admissions or retained completed results in a pass.

A parallel pass now admits at most 128 actions, or one configured worker wave
if it is larger. It refills free slots within that budget. At the boundary it
serves active workers' route handshakes, drains those workers, emits their
existing results and returns normally. A scheduled node-capacity re-probe
cannot reopen admission or cause a retry spin while the pass drains. Errors
and shutdown still use the existing supervision paths.

This preserves all results for a bounded `--once` JSON response, rather than
discarding completed outcomes. Recurring mode can release the pass-local
collections, apply its configured interval and reload config. Remaining items
are reconsidered through ordinary persisted ledger, claim and gate rules.
No claim or result is erased to force them through. Serial behavior is unchanged.

The bound is on admissions and completed result records, not bytes or elapsed
time. Large records, full-history ledger/event reads, active worker allocations
and slow workers remain separate concerns. A multi-hour live allocation profile
and idle/dispatch soak are still required for #1681.

## Native reproduction

The new real-CLI regression uses six local ticket/worktree jobs, a fake provider
CLI and fake forge. An explicit debug-only pass budget of four keeps it small;
release builds do not inspect that environment hook and use the production
budget. All ordinary node, routing, validation and claim admission still runs.

The retained previous CLI (`20368c67`) fails the test: it admits all six jobs in
the first pass, where four are expected. The repaired CLI passes: four jobs
finish in the first pass, the remaining two finish in the next, with no duplicate
executions. Both passes exit successfully. The focused repaired run took 11.59 s.

```sh
cargo test --locked --offline --test controller_refill_regressions pass_turnover:: -- --test-threads=1
```

The main implementation (`ec204e8a`) passed all **197 controller cases** and
**22 real-CLI refill cases** in 125.29 s, including fast-sibling refill, pressure
recovery/reprobe, shutdown, error handling and worktree isolation. Formatting
and all-target/all-feature Clippy with warnings denied passed. The log is
`pass-turnover-verification.log` under
`/home/khing/.cache/parasaur-instance-dispatch.9e0730tx`.

The final revision adds only an `active == 0` guard on the completion message:
shutdown cannot announce that workers have drained while the enclosing scope
is still joining them. Its focused turnover check passed again in 11.60 s.
The final revision also passed formatting/Clippy, 197 controller cases again and
both real-CLI shutdown cases (6.93 s). Its log is
`pass-final-verification.log` in the same cache. The retained debug CLI SHA-256 is
`3ddf89d9a5f69b6a6f4ce2751c2f1c4451b121c5e1b794ab09b19701d124b29e`.
The final matched assembly at
`/home/khing/.cache/parasaur-enterprise-current.d5ww2y_y/source` passed 1,093
server cases (37 explicit skips), 15 mock cases, typechecks, production builds
and private input/artifact/transformed transport guards. Its sibling `build.log`
records the run. Private runtime/web/SQL production source is unchanged; only
the core pin advances. Earlier dedicated isolation, browser and Auth runs
remain evidence for their recorded revisions and were not repeated here.
The 125-second local suite does not meet #1662's three hosted runs under 60 s,
nor establish the cause of its earlier hosted hang.

No installed release, live worker or hosted service was changed.
