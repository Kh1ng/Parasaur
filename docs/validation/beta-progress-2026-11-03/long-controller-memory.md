# Longer controller memory trial — 2026-11-05

The existing opt-in probe completed 48 real passes in one Debug controller with
four synthetic workers per pass: 192 provider starts, one controller PID, all
pass-drain assertions and a final config-driven graceful stop. The last active
sample occurred at 521.9 seconds; the complete test finished in 531.6 seconds.
The history began with about 32 MiB of unrelated ledger payload.

The retained current native host was SHA-256
`f7a4f687e4cbb3c1478b9495064deed3e637ad8c50a52e16a394bebe8c28712f`,
bound to public `cd67c949` and private `9d67290`. All tracked Rust sources/tests
were compared with its verified source before the harness was compiled. No
allocator override was set. The four-action pass budget and pressure fixtures
are existing test-only controls; this is not an optimized or production soak.
Only fake provider/forge executables, temporary repositories and a local bare
Git origin were used. Windows UI tests ran concurrently on the workstation;
this was not an isolated throughput benchmark.

| Sample | Resident memory |
| --- | ---: |
| Pass 1 | 68.6 MiB |
| Pass 12 | 151.9 MiB |
| Pass 24 | 167.9 MiB |
| Pass 36 | 167.0 MiB |
| Maximum active sample, pass 47 | 210.8 MiB |
| Pass 48 | 179.1 MiB |

The largest observed kernel high-water value at the samples was 335.4 MiB.
That is distinct from the paused-worker RSS samples and does not measure any
unsampled final-drain peak. Ledger growth between first and last active samples
was 2,457,729 bytes. RSS rose overall and also dropped, including near the end.
This does not establish a plateau, an unbounded leak or the live #1681 cause.
Earlier twelve-pass cases used another recorded binary and remain separate
experiments, not a controlled pass-count-only comparison.

An observer verified the owned PID/executable and read only its fixture paths.
It captured event/ledger sizes and `/proc` mapping totals for passes 29–48.
The event journal was 351,137 bytes at the first observation and 578,228 bytes
at the last. Those observations were asynchronous with the paused-worker
measurements: controller post-processing and child-process activity could occur
between reads. Mapping totals varied accordingly and cannot be treated as live
Rust heap allocations or freed-object counts. File size alone does not attribute
resident memory, allocator retention, object overhead or repeated history copies.

The next useful evidence is allocator/live-heap attribution at a consistent
lifecycle boundary, then repeated trials and an optimized multi-hour soak.
No production malloc setting or allocator change is justified by this one run.
Mac #1680 remains explicitly deferred and unmeasured.

The terminal passing log, all 48 samples, raw daemon stdout/stderr, metadata and
20 history observations are retained under
`/home/khing/.cache/parasaur-long-memory.k0pm1qrz/`. Reviewable copies and the
interactive RSS/high-water plot are in `artifacts/long-controller-memory/`.
The unchanged tracked harness is
`tests/controller_refill_regressions/ledger_retention.rs`; its required opt-in
result path and 48-pass bound remain in force. No live service, worker release,
provider credential or installed runtime changed for this experiment.
