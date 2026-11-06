# Native job policy and history context

This increment prepares and verifies the input for a central-owned native
factory job. It does not start a backend, refresh execution authority, transfer
a live claim or enable automatic native handoff. The receiving boundary remains
[native worker result ingestion](native-worker-result-import.md).

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

This is source-only. The check explicitly returns `execution_started: false`
and `authority_refresh_required: true`. A point-in-time context is not live
authorization. The next executor/broker increment must refresh central
authority, honor current local permission revocation, acquire admission on the
actual node, keep the original profile/work claims and native run identity,
and reconcile the same durable offer across disconnect/restart. It must not
fall back to the ordinary empty-history worker CLI or start a second controller.
No live factory result, source push, issue closure or controller restart is
claimed by this increment.
