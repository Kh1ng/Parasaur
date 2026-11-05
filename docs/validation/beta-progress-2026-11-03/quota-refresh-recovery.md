# Quota refresh recovery — 2026-11-05

A failed quota read retained its data but the page and navbar continued to
present it as current. Starting a retry also cleared the error before any
successful response arrived. A Windows store regression reproduced that loss
of uncertainty before this repair.

Same-scope retries now retain the failed-read state until success. Existing
profile/period isolation and rejection of late responses remain in place. The
page keeps recorded costs and allowances inspectable, adds an explicit warning
and retry, and replaces eligible/not-blocked claims with unconfirmed eligibility.
The navbar labels retained figures as last recorded, uses a neutral question
mark ring, and explains the unavailable read when opened. Historical windows
retain their values rather than becoming zero. Reset countdowns and pace
projections pause while the read is unconfirmed; recorded reset timestamps
remain inspectable. Successful recovery restores the fresh presentation.

Clearing all retained data and preserving it with explicit uncertainty were
considered. Preserving the recorded context lets operators inspect account
costs and prior limits while a worker or connection recovers. This only changes
observation: no allowance, admission policy, provider, claim or job is mutated.

Windows checks passed: 32 store/account-cost unit cases, the complete quota
failure → held retry → changed successful response browser flow, three existing
subscription browser cases, and web typechecking. Desktop and 390 px failure
layouts were inspected. Logs are retained at `artifacts/quota-recovery-*.log`.
The first test harness attempt lacked the mocked `getQuota` function; after
adding it, the old store reproduced the error-clearing failure. The corrected
production code passed the final checks.

This repair joins the [recorded account cost fix](account-cost-attribution.md)
in the matched `cd67c949` public package and `9d67290` private assembly. The
private production build, twenty assembled component cases and four Playwright
cases pass, including both account UI and held-retry recovery workflows. Its
fresh native isolation and real local Auth/relay checks also pass. Browser
checks used private transport local mode, not a real hosted session. The first
private component attempt began before copying the fixture completed, so no
cases ran; the completed fixture passed without changing app source.

The actual isolated release installer and all 29 JavaScript chunk body reads
passed, with bundle guards for the cost and unavailable-state copy. The idle
WSL user CLI is updated with rollback and both Windows integration reads pass.
The native desktop, settings and separate worker binaries retain their hashes.
The server/web changes are not deployed. Live/provider acceptance, signed
updates and complete admission-limit reconciliation remain required.
