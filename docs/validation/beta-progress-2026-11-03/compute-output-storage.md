# Saved-output storage controls — 2026-11-04

Private `ffb0c7b8e0be775fd31345b191f4e0133bd46e0f` adds storage-layer review,
bounded download and explicit discard primitives. Its public-core pin remains
`2cc5e3fd99ce3a4cc2165694712d014a3345b67d`. This is a focused implementation
increment, not a new combined-build or installed-app acceptance result. The
[previous full verification](enterprise-compute-reconciliation.md) remains the
authoritative complete assembly evidence.

The catalog reports retained, running and unresolved slots without exposing
host paths or reading archive contents. Unverified or absent archive sizes
remain unknown. Nonempty partial uploads are labeled incomplete. Unfamiliar
files, links and malformed ownership retain their capacity and refuse removal.

Download verification reads fixed 1 MiB blocks, computes an observed SHA-256
and per-block hashes, and permits at most two concurrent verification plans
per store instance. Blocks can be fetched separately without a whole-archive
buffer. These hashes describe observed download bytes, not producer attestations.

Explicit discard requires the selected slot incarnation, reviewed content hash,
stopped-producer evidence and a durable execution result. It hashes the archive
again before removal. This additional check addresses a reproduced same-size
rewrite whose timestamps could match the previous observation. Automatic empty
recovery still never removes saved output. Filesystem operations pin the original
directory inode so a stale or concurrent operation cannot delete a reused slot.

## Verification

After the authorized Ubuntu-24.04 restart, Linux filesystem read/write and access
to the Windows checkout passed. An isolated Linux copy of the changed modules
and their existing dependencies passed strict TypeScript checking. Its source is
`/home/khing/.cache/parasaur-isolation-dev.a4nylyqd`.

`node --import tsx --test cloudComputeStorage.test.ts cloudComputeWorker.test.ts cloudExecutionRecovery.test.ts`
passed **19 cases**, with **four explicit isolation skips**, zero failures.
The complete output is retained locally in
`artifacts/compute-output-storage-verification.log`. The eight new storage cases
cover closure/result requirements, reviewed hashes, cancellation, stale and
concurrent discards, slot reuse, links and unfamiliar data, incomplete archives,
block hashes and bounds, and verification concurrency. These checks use disposable
fixtures; no retained user output was removed.

## Still required

Owner-authenticated routes, mutation audit/idempotency, streamed client downloads,
explicit confirmation and Settings UI are not implemented by this increment.
Their authorization, dashboard relay and visual acceptance checks remain pending.
No release, deployment, installed app or factory worker was changed. Restarting
the distro stopped its Linux processes; this verification did not restart a
production worker.
