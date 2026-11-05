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
- All three subscription browser cases passed through the stateful sample
  server, including duplicate model candidates without duplicate account spend.
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

This source change follows the current `fdc9567f` package and `dec85e7` private
assembly. It is not yet included in those artifacts or the installed WSL CLI.
Matched packaging/private integration, real provider billing/usage ingestion,
quota-refresh recovery and complete admission-limit reconciliation remain open.
No provider login, billing call, live worker or hosted mutation occurred.
