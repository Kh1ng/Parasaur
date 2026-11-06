# Recorded factory spend ceilings

The finishing pass implements #1537 as an opt-in native intake policy. This
change is staged, not enabled on the live controller. No dollar amount has
been selected for the owner and no running service has been restarted.

`routing.spend_budget` accepts independent `daily_usd` and `monthly_usd` ceilings
with an explicit `actual` or `estimated` cost basis. Defaults can supply a
policy; a profile section replaces the entire policy and an empty section
disables inheritance. Invalid dollar amounts fail configuration validation.
The [operator configuration](../../OPERATIONS.md#factory-spend-ceilings)
documents scope and the exact settings.

Accounting shares the usage report's canonical execution boundary. Modern
per-attempt readings supersede their mirrored dispatch totals, including missing
attempt costs; historical dispatch summaries count once. Only proven unstarted
preflights and non-execution control records are excluded. Clearing attempts
does not erase spending. Missing or malformed cost/time evidence holds an
enabled budget rather than manufacturing a zero. Actual and estimated cost
never substitute for each other.

Status exposes each UTC window, its known subtotal, measured/unmeasured counts,
ceiling, reset and hold state. The loop selects a timed pause and resumes after
rollover; running dispatches finish normally. New dispatches recheck authoritative
ledger history before setup, and explicit CLI commands report a refused budget
before acquiring their claim. A hold is not a permanent work-item human gate.

One atomic receipt per profile suppresses repeat pause notifications. New
transitions record `spend_budget_held`, use existing channel delivery/failure
policy and do not wake a manager agent. The shared Overview renders both
windows and incomplete readings; effective profile configuration displays the
policy. An older node cannot invent an absent ceiling.

## Evidence

The final focused accounting/admission run passed twelve cases and the native
notification suite passed 44 cases. The real CLI scenario
target passed both daily/monthly cases and four support cases. It runs a fake
backend, retains that dispatch's full ledger, confirms the next loop launches
nothing, checks one pause notice, refuses a direct dispatch, then advances the
injected clock and observes the next backend launch after rollover. An initial
subscription fixture correctly lost hypothetical dollar cost during attribution;
the final fixture explicitly records cost through a standard execution route.

Two Windows rendered component cases pass, including missing costs, independent
actual/estimate labels, observed zero, absent policy and no 390px document
overflow. Desktop and narrow screenshots were inspected. Contracts build,
shared-web typecheck, production web/server builds and whitespace checks pass.
The broad native unit run passed 2,546 cases; its two installer failures were
caused by the runner omitting Node/npm from PATH and both passed after that
environment was corrected. Subsequent bounded-loop (20), prior-attempt (5),
lifecycle/review-generation (7) and spend (6) integration targets passed; their
counts include each target's support cases. Final all-target/all-feature Clippy
passed with warnings denied. Late-refusal and distinct feed-transition fixes
were verified by the final focused accounting, notification and spend runs.
Logs are retained in `/home/khing/.cache/parasaur-project-report.qy1ih6c9/`:
`spend-budget-final-checks.log`, `spend-budget-resume-checks.log` and
`spend-budget-final-admission-checks.log`.

## Acceptance limits and next factory boundary

This is a ceiling on recorded factory history on the executing node, not a
provider billing limit, a fleet-wide account budget or a reservation of future
cost. In-flight work can exceed it. Chat is outside this policy. Live acceptance
requires a matching controller binary before enabling any policy; replacing
only the CLI path does not update a controller already running the old inode.
The existing ten-hour memory observer and its controller were left running.

The native controller still executes locally. The existing fleet API starts a
separate full worker dispatch; it does not carry central native review/retry
history or this budget's authoritative ledger. Simply forwarding an action to
that API would omit policy evidence. The remote handoff must retain central
history, exact work/run/review-generation identity, claims and paid/review caps;
admit capacity on the actual execution node; and persist a terminal receipt
once before admitting follow-up work. An observation timeout must keep an
uncertain job held rather than dispatch a duplicate. No second empty-history
controller, capacity override or ownership reassignment was introduced.
