# Native worker result ingestion

The existing fleet session receipt reports process termination. It does not
carry the native dispatch's attempts, measured usage, validation, review or
publication evidence back into central's ledger. Running the full worker CLI
and treating exit zero as factory success would lose those facts and could
leave central admitting follow-up work against an empty history.

This increment implements the receiving side of that boundary. It does not
start remote execution or complete automatic native handoff.

`gah ledger receive-dispatch --binding-file <prepared.json> --receipt-file
<worker.json> --config-path <central.toml>` records a native result in the
configured canonical ledger. The operation is explicitly local-only and
security-sensitive in the capability manifest. A trusted central caller must
provide its independently prepared binding; a worker must never choose or
overwrite that binding through a generic remote command.

The binding names the canonical native run UUID, worker node, profile,
repository, work item, dispatch mode/reason, selected review generation and
explicit branch/backend/model/account constraints. It also names the digest
of the prepared central execution context. The receipt must match that digest
and identity. An explicit review requires its selected generation and current
review contract. Named account pins require canonical attempt identity evidence
unless no backend attempt started. Unsupported ledger schemas, wrong work or
source node, absent natural-exit evidence, negative transport sentinel codes
and nonterminal termination reasons are refused.

The digest binds the expected context identity; it is not proof that an
unintegrated worker consumed that context. The future sender/executor must
prepare, load and verify the central policy/history before launch. This CLI
does not authorize a launch, grant a paid route or approve publication.

The imported ledger entry retains its native business outcome and unknown
telemetry. Observed worker process termination is a separate additive
`remote_execution` field under ledger schema 12. Exit zero never changes a
failed validation into success. A read-only delivery profile retains its
`handoff` result and does not acquire a PR URL or publication flag.

## Lost acknowledgements and history

Import and duplicate detection share the canonical ledger's exclusive lock.
The importer streams and validates all existing history before writing; it
does not allocate another complete history vector. One original result digest
is stored in the imported row. It is computed from the original parsed worker
JSON before receiver defaults, normalization or schema upgrade. Formatting
whitespace does not change it. The sender must retain and forward that receipt,
rather than reconstructing history with new defaults or a lossy number format.
An identical retry returns `already_recorded`;
a different result for that native run is refused. This avoids a second
acknowledgement sidecar that could diverge if the process died after append.
The stored digest survives later central review backfill, so an old exact
receipt cannot overwrite corrected central fields or count another attempt.

A local claim may share the native run identity. An already recorded local
terminal dispatch cannot be replaced by an imported result. Unrelated rows,
attempt tombstones and existing review history remain in place. Corruption
anywhere in the journal refuses import without altering it. The ledger file
and actual parent directory are flushed before acknowledgement (including a
symlinked ledger target), including on replay
after a prior uncertain durability acknowledgement. A normal receipt is bounded
to 8 MiB on the CLI input path; an oversized result is retained for reconciliation,
not truncated or treated as a reason to rerun the job. Malformed worker JSON
produces a fixed schema error without echoing its contents.

## Verification and remaining boundary

Focused checks cover eight concurrent deliveries counting once, changed result
rejection, node/work/run/generation/account/context mismatches, malformed exit
evidence, central backfill before replay, a local claim versus a local terminal
collision, corrupt history, read-only handoff and unknown usage. A real CLI
scenario preserves prior history, imports and replays one worker result, refuses
a changed cost and checks that malformed private input is not echoed.

Final Linux checks pass: 132 ledger tests, nine CLI capability tests, 43
telemetry tests and seven CLI ledger cases (three shared fixture cases).
All-target/all-feature Clippy passes with warnings denied, as does formatting.
The source-identity regression imports a schema-11 receipt into schema 12,
keeps its original digest and replays it without another row. Logs are retained
in the isolated Linux source at
`/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-handoff-result-checks.log`.
The TypeScript contract builds and server typecheck pass.

Public source is `fbdaf459`. Private `59d8168` pins that revision; its complete
assembly passes 1,158 server cases (38 explicit skips), 17 mock cases, production
server/web builds, input/artifact guards and browser credential/transport checks.
Log: `/home/khing/.cache/parasaur-enterprise-current.bno66sx5/build.log`.

At 15:59:24 UTC on November 6, the original controller PID 666974 and observer
PID 667504 still matched their live units. The observer was running with 302
samples over 18,345.12 seconds, zero gaps and zero cgroup OOM kills. Latest
controller RSS was 733,252 KiB. This is not a ten-hour memory pass or evidence
of bounded memory under sustained agent load. Metadata is retained in ignored
`artifacts/central-live-20261105/native-result-import-memory.json`.

This is source-only. It is not deployed, has not imported a live factory result
and has not closed an issue. The running controller and its memory observer are
not restarted. Next, native job preparation must carry central policy/history
and run/review identity to the executor, retain the existing claim and admission
guards, and reconcile the same durable offer across disconnect/restart before
acknowledging result import and releasing follow-up work.
