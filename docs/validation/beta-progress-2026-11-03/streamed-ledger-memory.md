# Streaming ledger reads — 2026-11-05

Public core `5e261e0e9e8e975278f36579149c7954e60e4c6a` removes the
whole-file UTF-8 text copy from JSONL ledger snapshots. File reads now use a
buffered reader and one reusable line buffer. Human-gate transition checks use
the same reader under their existing exclusive lock; ordinary reads retain
their shared lock. Missing files still mean empty history. Invalid JSON, UTF-8
and read failures remain errors with path and physical-line context.

The parser preserves every decoded record and exact consumed byte counts,
including Unicode, blank lines, CRLF and a final line without a newline.
History is not truncated. Decoded history and the largest individual line still
consume memory proportional to their size; this is not a total memory bound.

## Verification and observed memory

`cargo fmt -- --check`, 119 ledger tests, 198 controller tests, and
`cargo clippy --locked --offline --all-targets --all-features -- -D warnings`
passed. Both the CLI and controller-refill harness built successfully. New
regressions cover physical byte counts, complete history, missing files and
strict corruption/read failures. Source and log:
`/home/khing/.cache/parasaur-streamed-ledger.a9kwm3ut/source` and sibling
`verification.log`. The exported source has no Git metadata; these results do
not claim a fresh repository-structure test run.

The existing opt-in controller probe ran twelve real pass boundaries in one
daemon with four synthetic workers per pass, 48 starts and seeded 32 MiB ledger
history. Both runs completed their config-driven graceful stop. Production
Rust source differed only in `src/ledger/jsonl.rs`; neither run changed the
allocator environment. Local fake providers and a local Git fixture avoided
real provider work.

| Debug run | First sampled RSS | Last sampled RSS | Kernel peak RSS |
| --- | ---: | ---: | ---: |
| Before, whole-file text copy | 145.5 MiB | 202.4 MiB | 401.4 MiB |
| After, streaming reader | 60.0 MiB | 153.1 MiB | 268.6 MiB |

This is one run per build. The after run still grows and jumps during warmup;
lower observed peak and final footprint do not prove the leak resolved.
Event-journal retention, live heap attribution, repeated trials, optimized
soaks and #1681's actual Linux OOM source remain open. Mac #1680 is deferred.
Reports, logs, chart data and the comparison chart are retained locally under
`artifacts/streamed-ledger-memory/`.

## Matched integration and local installation

Private `a10a5f2abd1f5b22d80cb810ec4c5fbb1e6d1dc7` pins public `5e261e0e`.
Its fresh assembly passed 1,105 server cases (38 explicit skips), 16 mock
cases, typechecks, production builds and private artifact/transport guards.
The refreshed Debug native host is SHA-256
`8412676624c9466025639ee7748ae874f76bdbaec7edb7ecb7cd584f12c6b8c7`.
All 100 dedicated native isolation cases passed (one platform skip). Fresh
disposable SQL replay and real local Auth/PostgREST/HTTP/WebSocket relay checks
passed, including queue/crash recovery, durable receipts and authorization.
Logs are siblings of
`/home/khing/.cache/parasaur-enterprise-current.9sb8cgdw/source`.

The [current public candidate](current-public-release.md) contains matched
optimized CLI/MCP, server and web assets. It passed 1,010 public server cases
(two skips), 16 mock cases, actual isolated release installation and all 29
JavaScript chunk body reads. The optimized CLI passed the 128 MiB journal
memory-limit regression and four review-format cases. Both binaries require
glibc 2.39. Checksums, ZIP CRC and every archived file's bytes were verified.

The idle WSL user CLI was atomically replaced and verified against the candidate:
SHA-256 `4cac1f30362fd072632f9dde9527183fff52a044e4335e308517b55949ab03e2`.
Rollback is `~/.cache/parasaur-upgrade-backups/gah-before-5e261e0e`.
The installed CLI passed the memory regression and both Windows desktop reads
(account metadata and Mistral preflight). Six worker-release binaries, the
native Windows executable and saved desktop settings retained their hashes.
No new user MCP installation or live deployment occurred. The later authorized
Ubuntu-24.04 restart recovered filesystem read/write access; it does not prove
long-running memory stability or restart interrupted factory jobs.

The [live central](live-central-8443.md) still lacks authenticated acceptance
evidence and its last observed dashboard asset GETs failed. This package is
unsigned and unpublished. Local integration does not establish beta readiness.
