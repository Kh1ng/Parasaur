# Bounded Rust output capture — 2026-11-05

Public core `2f8c9cf574dda08ca1820f9686da3ae202b0496c` fixes a reproduced
unbounded buffering path in the Rust runner. Private source `e7d088c53efed395aec3906ee3e30753aabd8aa2`
pins that core. This is local Windows/WSL evidence, not proof that the live
central's [#1681 OOMs](https://github.com/Kh1ng/Parasaur/issues/1681) share this
allocation source. The Mac report #1680 remains deferred.

## Reproduction and change

The shared backend/reviewer copier buffered a whole unfinished line with no
bound. Every read rescanned that entire buffer for a newline, producing quadratic
work as well as growing memory. Progress used an unbounded message queue, even
though supervisors only need to know whether output arrived.

Capture now permits at most 16 MiB of input and redacted output per stream per
invocation. It scans newly read bytes only, coalesces progress into one pending
notification, and continues draining after the limit without retaining more
output. Complete redacted lines already written remain available, followed by
an explicit omission notice. An oversized unfinished line is omitted whole;
splitting it could expose a secret across the cutoff. No project file is removed.

The copier reports completeness. A reviewer whose successful process has an
incomplete/error/oversized capture returns failure with no accepted stdout,
so a valid-looking APPROVE prefix cannot become review evidence. Old or reused
review log files also have bounded reads; an oversized file is not treated as a
complete prefix. Existing process/timeout outcomes and implementation execution
remain governed by their existing supervision rules. Capture truncation does
not itself cancel a running implementation. Logs may contain less diagnostic
output after this limit; this is deliberate and marked.

Implementation capture creates a persistent incomplete marker before launch.
Only a fully captured invocation can remove its own new marker; reusing an
already uncertain artifact preserves that uncertainty. Codex, OpenHands and
AGY summaries reject incomplete logs and omission notices. Diagnostic reads
are bounded too, so a large old log cannot recreate the allocation or supply
an apparently complete prefix. Separate authoritative transcript sources keep
their existing behavior. The marker is diagnostic metadata, not a power-loss
durability guarantee or an execution journal.

The limit is per stream/invocation, not a global disk-retention guarantee.
Implementation stdout/stderr append to one artifact, and previous attempts may
already exist. No claim is made that total daemon memory is 16 MiB: redaction,
other state, concurrent workers and allocator overhead are separate allocations.

## Isolated buffering measurements

Baseline probe: `/home/khing/.cache/parasaur-log-memory.bzsf_q6v`.
Final repaired probe: `/home/khing/.cache/parasaur-log-memory.0hvfmi6p`.
An optimized standalone Rust harness compiles the actual before/after copier,
generates newline-free input without allocating it, holds the progress receiver
undrained, and reads process `VmHWM`. Identity redaction deliberately isolates
buffering from regex/secret-registry overhead. These are synthetic copier-process
measurements, not the full CLI or the live central.

| Copier | Input | Peak RSS | Queued progress | Retained log |
| --- | ---: | ---: | ---: | ---: |
| Before | 32 MiB | 67,712 KiB | 4,096 | 33,554,432 B |
| After | 32 MiB | 18,688 KiB | 1 | 85 B |
| After | 64 MiB | 18,688 KiB | 1 | 85 B |
| After | 128 MiB | 18,688 KiB | 1 | 85 B |

The 32 MiB baseline took 18.53 s; the repaired runs completed in about 6 ms.
An earlier old-source 64 MiB run exceeded the probe's 60-second deadline and was
terminated; no completed RSS result is claimed for it. That record is retained
at `/home/khing/.cache/parasaur-log-memory.m0e4gnw7`. Source hashes in the completed
probe match the measured before revision and current repaired copier.

## Native verification

Rust source/cache: `/home/khing/.cache/parasaur-instance-dispatch.9e0730tx`.
Linux target: `/home/khing/.cache/parasaur-beta-target`.

- The runner/validation filter passed **228 cases** after capture and marker
  changes. The escaped-writer regression now requires failure for incomplete
  review evidence while retaining pipe closure, EPIPE and no-partial-line checks.
  After the final bounded diagnostic-reader changes, focused publication checks
  passed **11 output**, **five copier**, **one Codex incomplete/reused capture**
  and **four real-CLI review** cases.
- **34 review-workflow cases passed**, plus **four real-CLI review scenarios**,
  including a fake reviewer that prints an APPROVE prefix before 32 MiB of
  output. It fails and records no approval/verdict artifact. Normal review and
  bounded format repair still pass. All forge/provider executables are fixtures.
- **21 controller refill cases passed** again on the final committed source in
  114.60 s after restarting Ubuntu-24.04 (initial run: 114.15 s). This does not meet
  #1662's three-hosted-runs/under-60-second requirement or diagnose its old hang.
- `cargo fmt -- --check` and `cargo clippy --locked --offline --all-targets
  --all-features -- -D warnings` passed.

The debug CLI SHA-256 is
`fe07bdd295997b906aef70cb3a823ea081ea42dd70ec21d71819c60485e54e31`.
It is a test candidate, not the installed user/worker release. The retained
`output-memory-verification.log` contains workflow and initial refill results;
`output-publication-verification.log` contains final Clippy and publication checks;
`measurements.json` holds the isolated memory samples.

The exact private `e7d088c` / public `2f8c9cf5` assembly is retained at
`/home/khing/.cache/parasaur-enterprise-current.8d_rl8rh/source`. It passed
**1,093 server cases** (37 explicit skips), **15 mock cases**, server/web
typechecks, production builds, private input/artifact guards and transformed
browser transport checks. Its `build.log` records the complete run. Private
runtime/web/SQL source is unchanged from the preceding receipt-recovery pair;
only the core build pin advances.

After the owner-authorized Ubuntu-24.04 restart, this final assembled pair and
retained native host passed **96 dedicated isolation cases**, with one explicit
platform skip, in 35.64 s. The sibling `isolation-verification.log` records the
run. No job scopes remained afterward. Earlier SQL/Auth and browser records
remain evidence for their recorded revisions; they were not repeated here.

## Live acceptance remains open

#1681 identifies the Linux Rust loop, while #1680 has no Mac process attribution.
This change closes a definite reproduced capture path. It does not establish the
allocation source of the measured 4 GiB central, prove stable memory over hours,
bound every ledger/event/transcript reader, or replace a heap profile. Minute RSS
sampling across idle and normal dispatch, a live allocation profile and a long
soak remain required. No service drop-in, worker, issue, PR, hosted deployment or
installed application was changed, and neither memory issue was closed.
