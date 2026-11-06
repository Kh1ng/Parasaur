# Combined recorded usage

Public `39c29be49f38c2f2408ffe52e3ecc9946a408a73` is deployed to the
central API and shared web at `https://hermesagent.tail82695.ts.net:8443`.
Private `2b8a161bc973e8d8eaf9aca1dea240e778d73a87` pins that core revision
and passes its combined production build. Nothing was pushed or closed.

`GET /api/usage/report?profile=gah&days=7` combines the streaming native ledger
report with streamed durable chat history. Each source reports availability
independently. Missing readings retain known subtotals and measured/unmeasured
counts; they never become zero or complete billing totals. Native per-attempt
usage supersedes mirrored dispatch summaries. Older summary observations remain
explicitly legacy; their count may overlap recorded attempts.

The Telemetry card supports 7/30-day windows, backend/account/node, requested and
actual model, project, work item and UTC day grouping, and JSON export of the
loaded response including coverage. Exact identity includes auth binding and
quota pool. Missing historical project or node identity remains unknown.
Actual cost and API-equivalent estimates remain independent. Token categories
are never summed to manufacture total tokens or inferred cache usage.

Chat parsing bounds each event to 16 MiB without retaining transcripts. Exact
duplicate assistant events count once; conflicting sequence records or malformed
history make that source unavailable. Missing logs are empty recorded history.
Failed sources preserve successful readings with a visible coverage warning.
The existing chat-only endpoint and native telemetry exporter keep their
original scope; the new JSON export is the combined endpoint's response.

## Verification

- Seven native usage regressions and all-target/all-feature Clippy passed.
- Six server/HTTP cases cover combined same-day accounting, identity, missing
  readings, duplicate/conflicting events, source failure and query validation.
- Frontend unit suite: 181 cases passed; three final grouping cases passed after
  adding auth/pool identity. Two rendered component cases cover all grouping
  axes, JSON download, partial-source/error recovery and 1440/390px layouts.
- Seventeen mock cases, workspace builds/typechecks and whitespace checks passed.
- The combined private build passed 1,142 server cases with 38 explicit skips,
  17 mock cases, production builds and private credential/input/artifact guards.
- Optimized CLI inputs matched all 591 Rust/Cargo files. Installed CLI SHA-256:
  `b65fe387ff5e7fb809d766ac95ca5f468a00d97ccaa7cb168e0156d6cd4ae6b2`.

## Live acceptance

The guarded API/CLI installation completed at 13:16 UTC on 2026-11-06. API PID
756927 is healthy. The native controller remains PID 666974 running its existing
`742512` binary inode; it was not restarted when the CLI path was replaced.
The existing memory observer remains PID 667504. At 13:27 UTC it had 151 samples
and no gaps; the ten-hour observation remains incomplete.

At 13:20 UTC both sources were available for both windows. Seven days retained
46,602 reported tokens with six missing token observations. Thirty days retained
all 85 chat turns and a 30,640,172-token subtotal with 2,645 missing observations.
Both windows had zero actual-cost observations, so actual cost remained null.
Source-row measurements recombined exactly to the returned totals. Proven
preflight exclusions were 8,812 and 21,664 respectively. These are snapshots of
recorded central history, not complete provider bills or fleet-wide history.

The shared-web deployment completed at 13:25 UTC. All 30 asset files, including
29 JavaScript chunks, matched over HTTPS. Existing immutable chunks and the
credential bootstrap were preserved; the web rollout did not restart the API.
Owner configuration, credential, chat index, legacy handoff, ACP adapter, claims
and prior runtime backports were preserved. Hermes, worker and Mac were unchanged.

Machine evidence is retained under ignored `artifacts/central-live-20261105/`:
`recorded-usage-39c29be4-deployment.json`,
`recorded-usage-live-verification.json` and
`recorded-usage-39c29be4-web-deployment.json`. Build artifacts are under
`artifacts/parasaur-recorded-usage-39c29be4/` and
`artifacts/parasaur-canonical-usage-39c29be4/`.

The usage report acceptance for #1538 is demonstrated; the issue remains open.
Quota freshness, provider billing reconciliation, live GitLab persistent auth,
automatic native remote execution and sustained memory acceptance remain separate
work. Backlog still totals 38 filed open issues, or 29 scoped items including the
two unfiled usage/auth reports after excluding mobile, parents and parked work.
