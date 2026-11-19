# Worker ledger ownership — 2026-11-05

Public source `819937bf8c7e2b5530bf195cdd1e1015bf755726`, private pin
`b7c96bc580790e03c20b5f3c142ab1f1abbb472a`.

Parallel admission loaded a fresh full ledger and moved it into each worker for
the lifetime of the action. Most actions never used that history. Retry and
escalation only used it to produce the existing bounded prior-attempt context.
The pass also kept its initial full ledger/event observations alive. Serial
execution and the TUI likewise held history while executing their chosen action.

Workers now receive the already selected, owned context string: at most 4,096
bytes for retry/escalation, absent for new work. Full snapshots are released
before action execution in parallel, serial and TUI paths. Existing prior-context
selection still handles work aliases, related branch reviews, trusted artifact
reads and redaction. Persisted history, claim ownership, gates and route rules
are retained. Serial execution reuses its first observation without an extra
ledger read; after losing a claim it reloads evidence for the next candidate.
Stop reporting reads fresh events without an unused history parameter.

## Controlled real-CLI measurements

The tracked ignored probe is
`tests/controller_refill_regressions/ledger_retention.rs`. Each run generates 64
unrelated ledger records with 512 KiB summaries (32 MiB payload), then starts one
or four real fixture jobs. Fake backends wait on a sentinel while the test reads
the parent CLI's `/proc/<pid>/status`. Child/test-driver memory is excluded.
All launched jobs finish successfully after release. Admission pressure is an
explicit debug fixture; RSS and peak RSS are actual kernel readings.

| Active workers | Previous resident KiB | Repaired resident KiB | Previous peak KiB | Repaired peak KiB |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 71,352 | 38,040 | 136,164 | 130,324 |
| 4 | 224,592 | 96,548 | 465,700 | 335,292 |

Probe: `/home/khing/.cache/parasaur-worker-ledger-memory.j4f71d76`.
`measurements.json` and per-run logs retain the results. Generated ledgers are
equivalent fixtures with small differences in runtime metadata, not identical
byte streams. These are single-run debug measurements, not an optimized release
benchmark or a live central soak. Allocation/selection still reads full history;
the context bound does not bound total process memory or transient read peaks.

Previous CLI SHA-256:
`3ddf89d9a5f69b6a6f4ce2751c2f1c4451b121c5e1b794ab09b19701d124b29e`.
Repaired debug CLI SHA-256:
`f6028d54af0368758f144bb04794d16f2b71a51c5b600e7e94650c8a66e8d72b`.
The latter is retained beside the final private assembly as its native host.

To repeat on Linux/WSL with a built debug CLI:

```sh
GAH_TEST_LEDGER_MEMORY_RESULT=/absolute/output.json GAH_TEST_LEDGER_MEMORY_WORKERS=4 \
cargo test --locked --offline --test controller_refill_regressions \
  ledger_retention::active_workers_do_not_need_full_ledger_snapshots -- --ignored --exact --test-threads=1
```

Use one worker for the serial case. The probe uses fake providers/forge and
local Git repositories; no paid provider or real publication is involved.

## Regression and release checks

Formatting and all-target/all-feature Clippy with warnings denied passed.
**198 controller cases**, **16 prior-context cases**, and **22 real-CLI scheduler
cases** passed, with the controlled memory probe explicitly ignored in the normal
suite. The new unit check verifies owned bounded context after input history is
dropped. Existing single-pass ledger-read, gate deduplication, retry, claim,
pressure, shutdown, refill and worktree checks remain passing. The refill suite
took 126.92 s; this does not satisfy #1662's hosted/under-60-second requirement.
Log: `/home/khing/.cache/parasaur-instance-dispatch.9e0730tx/worker-ledger-verification.log`.

The matched private assembly at
`/home/khing/.cache/parasaur-enterprise-current.185brdy3/source` passed 1,095
server cases (38 explicit skips), 15 mock cases, typechecks/builds and private
artifact/input/transformed transport guards. Private implementation and public
server/web source are unchanged from the preceding diagnostics pair; only Rust
controller/TUI code and the private core pin advance. Earlier dedicated isolation,
Auth and browser checks remain evidence for their recorded revisions.

The [optimized public package](current-public-release.md) also passes isolated
release installation and cold asset reads. Its optimized CLI passes the event
memory regression and all four review-format cases. The idle WSL user CLI was
updated with rollback, and both Windows desktop installed-WSL reads pass.
Separate worker releases, the installed desktop and saved settings are unchanged.

No live central or worker service was changed. #1681 still needs actual heap/RSS
attribution and a multi-hour idle/dispatch soak; #1680's Mac report remains deferred.
