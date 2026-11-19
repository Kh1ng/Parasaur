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


## Allocator attribution follow-up

A separate Debug trial completed 12 real passes / 48 synthetic starts in one
PID, with four workers held at each sample. It used the same immutable `f7a4f687`
native host above; the exported harness source is public `6b101b98`, whose Rust
sources/tests are unchanged from `cd67c949`. The test passed in 135.61 seconds,
including pass-drain assertions and final graceful shutdown.

The owned controller alone loaded a diagnostic shared library. One added
observer thread waited on a private FIFO, then wrote `malloc_info(0, stream)`
XML after a 250 ms settle at each held-worker boundary. An exact executable
check excluded provider/Git children. No allocator setting, replacement,
`malloc_trim`, ptrace permission or installed application was changed.
[`malloc_info`](https://man7.org/linux/man-pages/man3/malloc_info.3.html)
provides versioned XML covering all allocator arenas and supports concurrent
calls. This diagnostic still perturbs scheduling and adds a thread/arena.

| Metric | Pass 1 | Pass 12 |
| --- | ---: | ---: |
| Paused-worker RSS | 64.1 MiB | 163.5 MiB |
| Allocator arena system bytes | 27.5 MiB | 124.9 MiB |
| Allocator reported free bytes | 26.2 MiB | 122.6 MiB |
| Derived allocator in-use proxy | 1.29 MiB | 2.33 MiB |

There were 19 arenas in every sample and zero direct mmap bytes reported at
these boundaries. The in-use proxy is arena system minus global free fast/rest
bytes plus direct mmap bytes. The parser checks XML version, free <= system,
and that per-arena system totals equal the global total. This proxy includes
allocator bookkeeping/caches and is not a precise count of live Rust objects.
Arena totals measure allocator-owned address space; they do not establish that
every free page is resident. They must not be stacked with or subtracted from
RSS as an exact resident-memory attribution.

The substantial reported free space is evidence that freed allocations retained
by the allocator contribute to this fixture's footprint. It contradicts treating
all sampled RSS as live application objects. It does not prove there is no leak,
identify the live #1681 OOM source, establish a plateau or justify a production
allocator setting. Repeated full-history decoding remains an allocation-churn
candidate; any scoped reader must preserve global routing/budget semantics and
corruption validation before replacing existing consumers.

A separate optimized single-pass/four-worker trial also passed using the actual
`6b101b98` release CLI (SHA `1834e100d031bc0ade4c95cf614b79faaae6be9e121f20b8c08de63a686f6942`).
Its paused sample was 45,248 KiB RSS, with a 157,300 KiB kernel high-water value,
28,823,552 allocator arena bytes, 27,498,451 reported free bytes and a 1,325,101
byte in-use proxy across 19 arenas. This is one bounded pass, not an optimized
repeated-pass soak or a controlled Debug-versus-release comparison.

The release cannot use Debug pressure hooks. The first attempt correctly hit
memory admission at three workers and timed out without a valid sample. A
second attempt with a 512 MiB fake-worker reservation correctly hit the default
two-core CPU reservation; it was stopped gracefully without a sample. The
successful owned fixture also set `worker_cpu_cores = 1`, retaining the real
adaptive memory floor, CPU ceiling and pressure checks. An intermediate fixture
macro syntax error was corrected before that successful run. No production
settings were changed. Owned fake shells from the first timeout were stopped;
the graceful stop and final successful trial completed their process cleanup.

Raw XML, samples, metadata, build/test logs and reproduction helpers are copied
to `artifacts/allocator-controller-memory/`, including the failed fixture attempts
under `optimized/`. Linux originals are
`/home/khing/.cache/parasaur-allocator-memory.3yr7kg6r/` and
`/home/khing/.cache/parasaur-optimized-allocator.217s_ok0/`.
Exact live-heap attribution, repeated controlled trials and an optimized
multi-hour soak remain required. Mac #1680 remains deferred.
