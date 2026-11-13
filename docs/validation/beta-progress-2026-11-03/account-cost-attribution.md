# Recorded account costs — 2026-11-05

The subscription cards and navbar summed `candidate.usage.actual_cost_usd`.
Those candidate summaries are grouped by backend/model and can fall back to a
backend total. They are not account totals: repeated model/pool candidates
could duplicate costs, and two named instances of the same backend could both
inherit the backend-wide figure. Three Windows unit checks reproduced those
failures before the repair.

The quota snapshot now adds optional `account_spend` rows. The producer uses
the ledger summary's canonical usage observations, which prefer observed
attempt usage over the dispatch aggregate. Each observation contributes once
to its recorded backend and `usage.backend_instance`; backend names and quota
pool strings never reconstruct an absent account identity. Unattributed history
stays in a null-instance row. The existing snapshot profile and `since` filter
apply before aggregation. This performs no extra ledger read.

Rows retain actual and estimated costs separately. Missing, non-finite and
negative observations do not become measured zero. An explicit zero remains
zero. The unknown counter counts recorded usage observations without an actual
cost, not every attempted execution. Executions without recorded usage are not
a claim of complete telemetry. Valid row totals reconcile with canonical
ledger cost totals; this is not provider invoice reconciliation.

The dashboard takes costs only from exact backend/instance rows matching the
account's declared scopes, independent of candidate count. Older workers without
the new field show unknown account cost. A null-instance cost is never assigned
to a default or named account. Cards and the navbar share the same cost label,
including the selected period, separate estimate and known missing-cost warning.
They explain that these are recorded executions in the current profile and
provider billing may differ. Provider quota windows remain separate. Routing
Usage details also qualifies its broader backend/model statistics and displays
actual and estimated costs independently instead of substituting one for the
other under a generic Cost label.

The worker relay accepts the optional extension while retaining v2/v3 support.
It rejects duplicate identities, unknown row fields, invalid labels/numbers,
inconsistent known/unknown counts and credential-bearing extensions before
returning the snapshot. The existing bounded response reader still applies.

Two approaches were considered: deduplicating candidate totals in the browser
or producing account totals from canonical ledger identity. Browser-only
deduplication cannot recover account identity or distinguish overlapping totals;
the producer now owns that accounting decision. No dependency or ledger schema
change was added.

## Verification

- Rust formatting and all-target/all-feature Clippy with warnings denied passed.
- 48 quota snapshot tests and 28 ledger-summary tests passed. Two new producer
  checks cover cross-account attempts, dispatch/attempt duplication, unassigned
  history, actual/estimated separation, measured zero and invalid/missing values.
- Ten real-CLI quota cases passed. The new actual `gah quota snapshot` case
  checks profile/time-window exclusion, named account costs, unattributed history
  and reconciliation with the snapshot's canonical total.
- Four new cost projection cases and eight existing Kanban unit cases passed.
- Fourteen Windows component cases passed, covering existing account/allowance
  behavior plus the shared cost cards/navbar and qualified routing history. Desktop and 390 px captures were
  inspected; the labels wrap without horizontal overflow.
- All three subscription cases passed; the full navbar/Quota workflow used the
  stateful sample server, including duplicate models without duplicate spend.
- All seven worker-quota relay cases passed, including scoped cost preservation
  and malformed/duplicate row rejection without echoing an upstream token.
- Contracts build, server/web typechecks and web production build passed. The
  web build retains its existing chunk-size warning.

Final Rust source/log:
`/home/khing/.cache/parasaur-account-cost.fqr5fd3q/source` and sibling
`verification.log`. This exported source has no Git metadata; no fresh
repository-structure suite is claimed. The initial compile identified a missing
`Clone` derive on the added row type; the corrected final checks above passed.
Windows logs are retained under `artifacts/account-cost-*.log` and the evidence
copy under `artifacts/account-cost/`.

The matched `cd67c949` public package and private `9d67290` assembly now include
this change and quota-refresh recovery. The full private build passed 1,106
server cases (38 skips), 16 mock cases, typechecks and artifact/transport guards.
Twenty assembled private component cases and four Playwright cases passed,
including the account UI and failed/held-retry/success quota workflows. These
browser checks used local mode; hosted browser acceptance remains open.
The refreshed native host passed 100 isolation cases (one platform skip), plus
fresh SQL replay and real local Auth/PostgREST/HTTP/WebSocket relay checks.

The optimized public build passed 1,011 server cases (two skips), 16 mock cases,
actual isolated release installation and all 29 JavaScript chunk body reads.
An additional actual installed CLI-to-HTTP quota check covered exact accounts,
unattributed history, actual/estimated/unknown/zero costs and profile/period
filtering. The test fixture initially used the wrong Rust config environment
variable and omitted the required profile artifact root; both fixture errors
were corrected before the final passing installer run. App source was unchanged.

The idle WSL user CLI is updated with exact candidate bytes and rollback at
`~/.cache/parasaur-upgrade-backups/gah-before-cd67c949`. Its memory regression
and both Windows metadata/preflight reads passed. Native desktop/settings and
six worker release binaries retain their hashes. No live service was updated.
Real provider billing/usage ingestion and complete admission-limit acceptance
remain open. The [current package record](current-public-release.md) includes
hashes, source pair, glibc requirement and deployment limits.
