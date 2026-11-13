# Repeated controller memory probe — 2026-11-05

The single-wave ledger probe did not establish behavior across repeated passes
in one daemon. The existing ignored probe now accepts
`GAH_TEST_LEDGER_MEMORY_PASSES` (default one, bounded to 48). Repeated mode uses
the existing debug pass-budget hook to admit one wave per pass, records the same
controller PID at each sample, and verifies real pass-drain boundaries. It then
disables only its fixture configuration and checks graceful controller exit.

Each wave pauses four synthetic provider processes after startup. The history
starts with 64 unrelated entries containing 512 KiB summaries, about 32 MiB of
payload. Filesystem/Git work is confined to temporary repositories and a local
bare origin. No real provider executable, service or remote forge is used.
Explicit pressure fixtures control admission; RSS and peak RSS come from the
real kernel `/proc/<pid>/status`. This is a Debug experiment, not optimized
release or live central profiling.

Daemon stdout/stderr go directly to retained files in repeated mode, avoiding a
full pipe or parent-side output buffer. A report is written after each sample;
partial reports alone are not proof that the run finished. The original one-pass
mode retains its measurement fields and passed separately after the extension.

## Results

Both 12-pass cases completed normally with 48 synthetic provider starts each, one
controller PID per run, four active workers per sample, verified pass boundaries
and a final config-driven stop. Both used the same retained runtime SHA-256:
`f6028d54af0368758f144bb04794d16f2b71a51c5b600e7e94650c8a66e8d72b`.

| Run | First sampled RSS | Last sampled RSS | Observed peak RSS |
| --- | ---: | ---: | ---: |
| Inherited glibc environment | 145.5 MiB | 202.4 MiB | 401.4 MiB |
| Process-local `MALLOC_ARENA_MAX=1` | 127.4 MiB | 165.5 MiB | 290.9 MiB |

The last active-worker samples occurred around 123 seconds into each run. The
ledger grew by about 0.55 MiB in each case. Event-journal growth and live heap
allocations were not measured. These are one run per condition, without repeated
trials or throughput/load conclusions. The one-arena observation is diagnostic,
not an application or system configuration change and not a recommended fix.

RSS increased in both traces. The difference is consistent with allocator
behavior contributing to footprint, but does not isolate retained Rust objects,
fragmentation, history growth, or the live OOM cause. Existing whole-ledger text
buffers remain an inspection candidate; this probe does not prove they caused
the observed growth. A longer soak and heap/event-history attribution remain
required before calling the memory problem resolved.

## Verification and evidence

Cargo formatting, the targeted test build, the original one-wave probe, both
12-pass cases and targeted Clippy with warnings denied passed. Only opt-in test
code changed; no production allocator, controller or installed binary changed.

The owned Linux source and complete logs are under
`/home/khing/.cache/parasaur-repeated-memory.ua1tx8rj/`.
Reviewable copies of the reports, build/test logs and interactive RSS comparison
are retained in `artifacts/repeated-controller-memory/` on Windows. The runtime
is the retained host under the preceding verified enterprise assembly; it was
not rebuilt for this experiment. The probe source was built separately with
Linux dependencies and a shared Cargo target directory.

An example invocation in an owned Linux test checkout is:

```sh
CARGO_BIN_EXE_gah=/absolute/path/to/verified-debug/gah \
GAH_TEST_LEDGER_MEMORY_RESULT=/absolute/path/to/report.json \
GAH_TEST_LEDGER_MEMORY_WORKERS=4 GAH_TEST_LEDGER_MEMORY_PASSES=12 \
cargo test --locked --offline --test controller_refill_regressions \
  ledger_retention::active_workers_do_not_need_full_ledger_snapshots \
  -- --ignored --exact --test-threads=1
```

Repeated mode requires the Debug fixture hook and rejects a run without real
pass-boundary evidence. It is not a production benchmark. MacBook #1680 remains
deferred; the confirmed live Linux OOM #1681 remains unattributed. Fresh Windows
[central checks](live-central-8443.md) still find asset failures and unavailable
protected access; no live job, claim, review gate or host setting was changed.
