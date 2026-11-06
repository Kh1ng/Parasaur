# Native job policy and history context

This increment prepares and verifies the input for a central-owned native
factory job. It does not start a backend, refresh execution authority, transfer
a live claim or enable automatic native handoff. The receiving boundary remains
[native worker result ingestion](native-worker-result-import.md).

## Bounded complete-history chunks

The actual central probe below exposed the single-file history cap. Context
version 2 now carries an explicit ordered chunk count and fixed names
`history-0000.jsonl` through `history-0007.jsonl`. Each file retains the 128 MiB
bound; eight chunks impose a 1 GiB total ceiling. Ledger rows stay intact and
in append order. No row selection, review/spend reset, compression dependency
or whole-history memory buffer is introduced.

Each chunk is private, hashed, fsynced and committed without overwriting an
existing file. The ready manifest is published only after the complete source
scan succeeds. Exceeding the row/chunk limit fails with partial files retained
for reconciliation, never a ready partial context. Version 1 contexts are
rejected; that candidate was staged only and no live broker consumes it.

The worker requires the exact contiguous chunk set and verifies each digest
while streaming into an atomic job-ledger write. Missing, reordered, changed
or extra chunks fail before a usable job ledger is committed. It then verifies
the complete entry count/profile/schema through the normal history contract.

The shared filesystem-capacity check also accounts for the additional write:
central preparation checks the maximum 1 GiB context allowance, and worker
materialization checks the manifest's actual history bytes. Both preserve the
existing 10 GiB dispatch free-space floor. This is a preflight, not a space
reservation or execution admission. Existing dispatch callers reserve zero
additional bytes and retain their original floor.

Verification passes 11 focused context/history cases, the actual CLI context
case and all-target/all-feature Clippy with warnings denied. Cases include
cross-chunk preservation of review and paid grant/revocation rows, missing and
reordered chunks, invalid counts, overflow, no overwrite, private permissions
and reservation arithmetic above the filesystem's available capacity. Logs:
`/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-handoff-context-final-checks.log`.
The earlier chunk implementation also passed ledger, dispatch, capability,
telemetry and daily/monthly rollover checks; the final disk-preflight changes
were verified by the focused cases and all-target Clippy. The production
rerun below now clears the size blocker; worker runtime compatibility and
execution remain incomplete.

## Production history transfer — version 2

Public `88f27cf2` was built from an immutable archive with all 606 Rust/Cargo
inputs verified. Its optimized binary SHA-256 is
`57938fbae6cfe8ba155659539164c945b03ce7d8551ff0c0fab62d2bff6ed29d`.
Production context and zero-dollar spend refusal smokes pass. The artifact
is `artifacts/parasaur-native-context-88f27cf2/`; build log:
`/home/khing/.cache/parasaur-native-context-release.80roa8b_/build.log`.
It remains staged, not installed.

At 17:24 UTC on November 6, the candidate prepared the actual central profile
successfully: 26,302 entries, two chunks and 249,649,064 history bytes. The
complete manifest digest is
`5653722872101db7b402bab2b8eebc4cc9bcd01b2337c40bc2633fb36fada023`.
The 4,916,683-byte transport archive was SHA-verified before extraction into a
new private WSL directory. Fixed member names, file types, duplicate names,
total size and extracted ownership permissions were checked. No Mac transfer
or model/issue execution occurred.

The actual Windows WSL worker check then refused the context because its
`claude_args` differ from central's runtime-rule digest. Separate inspection
also confirms differing `opencode_args`. The helper selected the same central
config as the live API (`/srv/gah/config/config.toml`), ruling out a diagnostic
config mix-up. Central explicitly has `--safe-mode` for Claude and the packaged
reviewer-agent override for OpenCode; worker vectors are empty. These arguments
can affect runtime permissions. They were neither discarded nor automatically
copied, and no worker compatibility/execution pass is claimed. Resolve the
runtime requirements and their actual executable support before another check.

API PIDs 841661 (central) and 352938 (worker), central controller 666974 and
observer 667504 were unchanged. Both probes verify configuration and installed
CLI hashes stayed unchanged. Saved reports are
`artifacts/central-live-20261105/native-context-88f27cf2-central.json` and
`native-context-88f27cf2-worker.json` in the same ignored directory. Owned remote
and WSL context directories retain this exact diagnostic run for reconciliation.

Matched private `98f2e7e` pins public `88f27cf2` and passes 1,158 server tests
(38 explicit skips), 17 mock cases, production builds, private artifact/input
guards and browser credential/transport checks. Log:
`/home/khing/.cache/parasaur-enterprise-current.dmqcov66/build.log`.
Its Supabase values are fixtures, not live SSO evidence.

## Runtime argument compatibility

The launch-code trace confirms that both OpenCode implementation and review
select their agent from the job role and remove profile `--agent` overrides.
The native policy now uses that same shared removal helper before hashing
OpenCode runtime arguments. It still hashes model pins, which can influence
routing before launch, and all other runtime/permission flags. Other runners
retain their exact argument-vector checks. No broad normalization or unknown
flag removal was added. Context cases accept ignored agent overrides and
refuse differing model, permission and output arguments before job creation.

Both installed Claude CLIs expose central's `--safe-mode` in their own `--help`.
With the real worker WebSocket roster at zero active sessions and its native
loop inactive, the worker's empty `claude_args` was set to `["--safe-mode"]`.
An in-memory full TOML comparison verifies every other configuration value
remains unchanged. The original config is backed up privately under
`~/.local/state/gah/worker-recovery-20261106/native-runtime-safe-mode/`.
The API remains PID 352938 and its CLI hash is unchanged; no service restart
or model/job execution was used. Ignored metadata report:
`artifacts/central-live-20261105/worker-native-safe-mode-alignment.json`.

Twelve focused context cases, the actual CLI check and all-target/all-feature
Clippy with warnings denied pass for the shared-filter change. Production
context re-preparation and worker acceptance of this candidate remain required.

The `69e2219d` optimized candidate passed real central preparation with 26,339
rows (249,965,303 bytes in two chunks). With Claude aligned, the actual worker
check next refused its missing OpenHands profile. Central's `nous-hy3` profile
uses a central-only loopback endpoint, so the provisioning guard refused it
before writing a credential or changing the worker configuration. This is not
evidence that OpenHands is available on the worker.

For a job with an explicit supported backend and non-default model, compatibility
now compares the runtime that job can actually execute. Existing dispatch routing
preserves both selections through initial routing and retries, including when
fallbacks are configured. OpenHands jobs still require both OpenHands arguments
and the profile to match; aliases retain that requirement. Auto, unknown-backend,
missing-model and dynamic/default-model requests retain all runtime checks.
The trusted manifest binds the exact request: a future executor must dispatch
that request unchanged and cannot use this context to switch providers/models.
This supports heterogeneous workers without requiring credentials for unrelated
providers. It does not implement automatic central route selection or execution.

The pinned-route cases also exercise refusal of a selected runtime's changed
permission flags and show that automatic/unpinned jobs cannot take this path.
The model/permission/output and ignored OpenCode-agent cases use the actual
selected OpenCode backend. The earlier launcher regression run passes 228 cases
and four explicit-route cases; its log is
`/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-runtime-argument-regressions.log`.

## Central preparation

The private `native-handoff prepare` command accepts a trusted request file,
selected execution node, central URL, central configuration and a new output
directory. `JobRequest.args` carries the actual native dispatch arguments,
including run/work identity, retry evidence, explicit flags, branch repair
context and the expected review generation. Process-local admission channels
are not serialized. Reviews require their selected generation. Absolute ticket
paths are made relative to the central repository; escaping paths and implicit
current-branch execution are refused.

The context contains a versioned manifest, request, portable policy, complete
profile ledger history and optional repository permission policy. Its binding
names the manifest digest. History is streamed in append order, preserving
review attempts, paid grants/revocations, attempt tombstones, older repository
identities within that profile and unknown usage. Other profiles are excluded.
The canonical ledger remains unchanged. Unsupported history schemas or invalid
history prevent a ready manifest from being published.

Portable policy explicitly classifies profile fields. New unclassified fields
refuse transfer instead of silently acquiring a worker default. Effective
central routing, review/paid/issue/spend limits, publishing rules, validation,
timeouts, model selection and context budgets travel with the job. Central
executable/state/environment-file paths, credential references, notification
commands and manager wake configuration are not copied. Backend argument and
OpenHands profile rules are pinned by digest: the worker must have matching
values, since arbitrary CLI arguments can contain permission restrictions.
The context does not silently discard central flags or merge opposing options.
Recognized secret material in portable policy/request input is refused; ledger
history uses the normal redaction boundary.

## Worker verification

`native-handoff check` reads the actual configured node identity and requires
the expected manifest digest from the central offer. The configured worker
central URL must match. Context/ledger/review-contract versions, required fixed
filenames, file ownership, private permissions, byte limits, hashes, history
scope and record count are checked. A missing history file is an error; it is
never replaced with an empty ledger. A deliberately empty, verified central
history remains distinguishable from a missing one.

The worker receives central project policy while retaining its own repository
and executable paths, account credential/state configuration and node-capacity
settings. Matching named accounts keep local runtime bindings. Missing,
disabled, mismatched or model-incompatible accounts stay disabled; a node does
not need every account declared on another node. Worker-only routing defaults
cannot expand the prepared central candidate policy. Neither this operation
nor the context turns off the native admission guard.

Central and worker repository permission files are intersected. The checked
copy cannot permit an action refused by either input. The worker's global
ledger is not overwritten: verified history is materialized in a new private
job directory. Inherited ledger/event/reconciliation overrides are cleared in
the private check child; the library also refuses conflicting overrides. The
check reads the effective ledger through the ordinary ledger reader to prove
that the job sees its prepared history rather than an empty worker store.

Both commands are hidden, local-only, security-sensitive capability operations.
They do not expose filesystem paths through generic remote commands. Existing
destinations are not overwritten. Partial inputs/work directories are retained
for reconciliation; they are not permission to retry execution. JSON inputs
are bounded at 8 MiB, profile history at 128 MiB and repository policy files at
1 MiB. File data and directory entries are flushed before success is returned.
These are input bounds, not a replacement for node-wide archive retention or
execution admission.

## Generation and failed-result handling

A review with an expected generation now verifies its freshly resolved source
before reviewer routing or launch. A changed/missing generation becomes a
`stale_source` preflight result, retaining the observed generation and no review
opinion. It does not invent a validation result for validation that never ran.
Native dispatch records explicitly requested model/branch metadata before
workflow preflight, so an early failure does not lose those known facts.

Result import retains valid preflight failures and typed policy refusals even
when no observed generation or route exists yet. A stale-source failure is not
relabeled as an opinion on the requested generation. A read-only delivery can
report its failed original mode, but cannot claim remote publication. Proven
zero-attempt policy refusals do not invent unknown execution spend; once a
backend may have run, missing cost still holds an enforced budget.

## Evidence and remaining execution work

Focused tests cover policy/history/spend preservation, private runtime mapping,
heterogeneous and disabled accounts, immutable directories, altered/missing
history, wrong node/origin/digest, path escape, selected generation, conflicting
permission arguments and known unstarted versus possible-invocation accounting.
A real CLI prepare/check scenario deliberately supplies an empty inherited
worker ledger, sees the one central history record through the effective
reader, and proves that no provider executable was probed or launched. Tampered
history is refused without echoing its private canary.

Linux checks pass: seven context cases, 134 ledger cases, nine capability cases,
43 telemetry cases, seven CLI ledger cases, one actual CLI context scenario,
570 dispatch cases, twelve spend cases and six spend-ceiling integration cases.
These are counts per filter, with overlapping cases and shared fixtures; they
must not be summed as distinct tests. Daily/monthly rollover integration and
all-target/all-feature Clippy with warnings denied pass. Formatting passes.
The final runtime-rule diagnostic check identifies the mismatched field without
echoing its arguments, so configuration drift is actionable without exposing
its contents. Its context/actual-CLI checks and Clippy pass again.

Logs are
`/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-handoff-context-checks.log`
and `native-handoff-context-final-checks.log` in the same directory.

Public `6b67abe1` is pinned by private `5385b42`. The matched assembly passes
1,158 server cases (38 explicit skips), 17 mock cases, production builds,
artifact/input checks and browser credential/transport guards. Log:
`/home/khing/.cache/parasaur-enterprise-current.i6_q6u2s/build.log`.

An optimized candidate was built from a fresh immutable public archive, with
all 605 tracked Rust/Cargo files compared. It passes production native context
prepare/check (including inherited-empty-ledger refusal/replacement by prepared
history) and a zero-dollar spend refusal before any dispatch session. No model
or real issue was selected. Artifact:
`artifacts/parasaur-native-context-6b67abe1/`; SHA-256
`12bdd03a63764394caae4b0e3a124612684d60f89406682a1973af2d40c6fbe1`.
It reports 0.3.0, requires glibc 2.39 and is not installed. Build log:
`/home/khing/.cache/parasaur-native-context-release.ot9eh4h3/build.log`.

At 16:59:57 UTC on November 6, controller PID 666974 and observer PID 667504
were unchanged and live. The existing observation had 363 samples over
22,005.309 seconds, zero gaps and zero cgroup OOM kills. Latest controller RSS
was 733,700 KiB. Ten-hour and sustained-agent-load acceptance remain incomplete.
The metadata report is retained under ignored
`artifacts/central-live-20261105/native-context-memory.json`.

## Actual central compatibility probe — November 6

The staged candidate was checked against the running central API's actual
configuration and inherited environment, using an owned diagnostic request
with no issue or model execution. Preparation failed with
`Native profile history exceeds its context limit; retain history for reconciliation`.
The canonical ledger is 261,588,311 bytes; the context currently caps its
selected-profile serialization at 128 MiB. A raw ledger size and its normalized
profile projection are different measurements, but the actual preparation
failure proves that the latter also exceeds the context cap.

No complete context manifest or archive was published. Therefore worker
compatibility was not tested, and the synthetic release smoke is insufficient
to claim compatibility with the production history. The next implementation
must accommodate complete history with bounded transfer, disk and memory use;
it must not discard rows, start with empty history, or bypass spend/review
guards to make the probe pass.

The API remained PID 841661, controller PID 666974 and observer PID 667504.
Configuration and installed CLI hashes were unchanged. The candidate remains
staged, not installed. Private failure diagnostics and metadata are retained
on central under `~/.local/state/gah/native-context-probe-6b67abe1/`.
Reinspect that saved diagnostic rather than retrying the same incomplete
directory. No service was stopped or restarted.

This is source-only. The check explicitly returns `execution_started: false`
and `authority_refresh_required: true`. A point-in-time context is not live
authorization. The next executor/broker increment must refresh central
authority, honor current local permission revocation, acquire admission on the
actual node, keep the original profile/work claims and native run identity,
and reconcile the same durable offer across disconnect/restart. It must not
fall back to the ordinary empty-history worker CLI or start a second controller.
No live factory result, source push, issue closure or controller restart is
claimed by this increment.
