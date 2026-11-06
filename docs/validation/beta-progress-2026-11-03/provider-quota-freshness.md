# Provider quota identity and freshness

## Live recheck after the owner's usage report — November 6

At 17:07–17:08 UTC, the actual tailnet dashboard HTML and all 30 assets
matched the deployed `b27bdf55` manifest, which includes the identity/freshness
corrections below. The Windows desktop's saved central URL also points to
that dashboard on port 8443. This proves the served build, not the contents of
an already-open webview or the GitLab panel.

The live snapshot still contains a Nous Portal monthly reading observed on
October 22 with an October 25 reset and zero percent remaining. Running that
snapshot through the current subscription selector keeps it under
`opencode:nous-portal-api` / Nous, with no current allowance and no reset
countdown. Mistral's `opencode` account has no quota windows. The screenshot's
Mistral/expired-monthly presentation is therefore not reproduced by the
currently served build and live snapshot. The particular running client still
needs verification; a central asset check cannot establish that acceptance.

Collection remains incomplete: the source audit contains six failed checks,
four no-data checks and one sign-in-required check. Historical observations
and missing billed costs must not be presented as current provider allowance
or reconciled billing. No provider login was changed during this check.

Ignored evidence: `artifacts/central-live-20261105/quota-served-current.json`
and `quota-current-verification.json` in the same directory. The former binds
the served assets and the fetched snapshot; the latter records selector
assertions against that snapshot. Neither establishes provider-bill accuracy
or real browser acceptance.

The live snapshot showed a Mistral OpenCode routing candidate under `opencode`
and a Nous Portal account reading under `opencode:nous-portal-api`. The
subscription derivation removed every colon suffix, combined those unrelated
sources, and chose the candidate's Mistral label for Nous's monthly quota.
The source also retained numeric windows on checks whose latest status was
failed. A future reset date alone did not establish a current allowance.

Source `4142d107` preserves full instance identities, apart from Antigravity's
documented external/Google-native pool suffixes or an explicit named credential.
Latest checks match exact backend, credential, instance, pool and applicable
model. A failed/no-data/unconfigured/login-required check qualifies the retained
reading; another account's check cannot affect it. A newer check does not change
an old observation's age. Undated, invalid, future, over-30-minute-old and ended
windows remain historical. The 30-minute threshold is the existing UI policy.
Measured percentages and ledger costs are retained; no new allowance or zero
usage is inferred. Fresh sibling pools can still select a current limit.

The navbar, account cards, routing rows and Kanban share these rules. The clock
reads current time on any render as well as its 30-second tick, so a newly
received observation cannot be rejected merely because it arrived after a
stored clock value. Historical windows show their reason and observation time.
Recorded actual/estimated costs retain their separate provenance.

Validation: 23 focused unit tests, 18 component tests, five quota/subscription
end-to-end tests, typecheck and production build pass. Two dated positive-case
fixtures were corrected to use a future reset. The first end-to-end run also
caught the stale-render-clock defect; the corrected clock makes recovery and
initial load pass. Private assembly `ed90441` pinned to `4142d107` passes 1,106
server tests with 38 explicit skips, 16 mock cases, production builds/typecheck,
private input/artifact checks and transport checks.

Source `88e5d16d` additionally preserves the older central's tab-only handoff
response during separate web rollout. Eight GitLab end-to-end tests cover both
protocols. A paired-device response takes precedence and never copies an owner
token; a legacy response remains in sessionStorage only, absent from another
tab and localStorage. The persistent-cookie server implementation stays staged
until the GitLab proxy can pass its cookies. This compatibility path does not
finish authentication continuity on the old server.

Follow-up `99f8cca1` excludes anonymous native `:usage-probe` markers from
subscription accounts while retaining named credentials. Latest-observation
selection compares timestamp instants, including differing ISO offsets.
Regression cases cover both boundaries.

Shared web `99f8cca1` is now live. The manifest in local ignored
`artifacts/parasaur-web-99f8cca1/` binds the immutable Git archive, verified
unchanged dependency lock, no source maps and all asset hashes. The central
verified every served asset, retained its HTML bootstrap and API PID, and did
not restart or replace the controller. Native visual acceptance, reconciliation
against actual provider bills and repair of failed provider logins remain open.

Final private assembly `c4b5630`, pinned to `99f8cca1`, also passes 1,106
server tests (38 explicit skips), 16 mock cases, server/web production build,
typecheck, private artifact/input guards and credential transport checks.
Its isolated WSL build log is
`/home/khing/.cache/parasaur-enterprise-current.986fg7be/build.log`.
