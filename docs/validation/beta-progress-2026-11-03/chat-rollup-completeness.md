# Chat rollups preserve missing measurements

Public `83562e8a` corrects a second layer of usage accounting. The earlier ACP
fix retained token categories on individual turns, but the report still filled
missing input/output counters with zero, reconstructed missing totals from those
partial counts, and excluded usage-less turns from grouped account and ticket
totals. Telemetry then labeled the arithmetic difference between total and
input/output as cache usage. That difference is not a provider-reported cache
measurement and is not portable across provider accounting conventions.

The rollup now retains every in-window assistant message in its backend,
account, model and UTC-day group. Each counter and API-equivalent estimate needs
a valid nonnegative reading from every included turn to yield a complete sum.
A missing reading invalidates that field alone. Explicit zero remains measured
zero. Cache read, cache write and reasoning stay independent of input, output
and total; the provider's total is never reconstructed. Future and invalid
timestamps are excluded from the bounded window.

Ticket totals follow the same rule. `unattributed_turns` and
`usage_unavailable` identify a subset of turns already included in the rows;
they must not be added to the new rows' turn counts. Cost-only observations
survive even when the turn has no token counters. Unknown category fields on
older servers remain unknown in the page.

The Telemetry card reuses the session drawer's complete-reading aggregation
rules when combining day rows. It shows explicit cache/reasoning columns and
suppresses numeric progress bars for unknown totals. Backend/account grouping
uses structural keys rather than presentation labels. This preserves the
existing surface while correcting misleading numbers; a replacement visual
design was unnecessary for this accounting defect.

## Verification

- Contracts build, server/web typechecks and whitespace checks passed.
- Eight server and HTTP cases cover measured, missing, partial, zero, invalid,
  cost-only and overlapping category readings, ticket totals and time bounds.
- Five frontend unit cases cover the shared session aggregation and day/account
  rollups. The rendered component check passed at desktop and narrow widths,
  including incomplete bars, exact cache categories, zero cost and window changes.
  Screenshots were inspected; the first test run's narrow-screen cell-index
  assertion was corrected to inspect the desktop columns at desktop width.
- Private `c23945c` pins the fix. The combined private build passed 1,123 server
  cases with 38 explicit skips, 16 mock cases, production builds and private
  input/artifact/browser credential guards. Log:
  `/home/khing/.cache/parasaur-enterprise-current.oidcj558/build.log`.

## Live acceptance, November 6

All 31 served JavaScript/CSS assets from `83562e8a` were byte-verified through the
Tailnet address. The central API's single `managerChat/usageRollup.js` module
reproduced its installed preimage before replacement. An all-session idle
check preceded the API restart, PID 687517 to 696154. Node identity, chat index,
config, credential, CLI and legacy GitLab handoff were preserved. Code and web
rollback copies remain in the private central repair directory.

At 11:40 UTC the old `gah` 30-day response had three grouped turns and 82
usage-less turns outside those groups. At 11:41 UTC the new response retained
the full 85-turn count, with the same 82 missing-usage diagnostics, and its
incomplete total was null. The seven-day window was empty before and after.
This proves the observed count was retained, not that historical missing
measurements were recovered. No billing values were invented.

Native controller PID 666974 and observer PID 667504 remained unchanged. At
11:41:53 UTC the existing ten-hour observer was running with 50 samples and zero
gaps. The soak is still incomplete. The Windows worker, MacBook and Hermes
service were not changed during this rollout.

This is a correctness prerequisite for #1538, not its completion: combined
chat/dispatch spend, cross-node reconciliation, project/work-item report
grouping and telemetry export acceptance remain open. Provider quota availability,
actual subscription billing and persistent GitLab authentication are separate
unfinished requirements.

Build archives and hashes are under ignored
`artifacts/parasaur-usage-rollup-83562e8a/`; metadata-only live reports are copied
under `artifacts/central-live-20261105/`. No private prompts or responses are in
those metadata reports.
