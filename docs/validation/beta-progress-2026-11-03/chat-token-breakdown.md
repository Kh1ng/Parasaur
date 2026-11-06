# Chat token categories and incomplete totals

The ACP client retained input, output and total tokens but discarded the
provider's cache-read, cache-write and reasoning counters. This made the total
harder to interpret. These categories may overlap input/output; their sum is
not an independent replacement for the reported total. The installed ACP SDK
schema documents cumulative counters and nullable optional categories; see the
[ACP schema](https://agentclientprotocol.com/protocol/v1/schema).

ChatUsage now carries optional cache_read_tokens, cache_write_tokens and
reasoning_tokens through durable events, history and live replies. The adapter
differences each counter separately. Missing or invalid counters remain null.
An optional cumulative counter without a previous reading cannot establish a
turn delta. Explicit zero remains zero, and a counter reset uses the new reading.
An absent report or failed turn invalidates the session baseline: the next
report reestablishes it without attributing several turns to one reply.
The independently reported total is unchanged. Existing logs remain compatible;
discarded historical categories cannot be reconstructed.

The session detail drawer displays all three categories. Its summary previously
omitted replies with no usage and summed whatever values remained, making a
partial reading look complete. A session total now requires that field from
every assistant reply. Missing token totals and costs have explicit explanatory
text. No quota, price, billing class or provider allowance is inferred.

Verification on 2026-11-06:

- 60 focused server tests pass in Ubuntu-24.04, including cumulative and
  missing-report recovery over child-process ACP connections and durable replay.
- 43 adapter/conversion tests also pass on Windows. The existing session-log
  suite's POSIX path-suffix assertion fails on Windows and passes in WSL.
- 17 web unit cases pass, including subscription identity/freshness regressions
  and missing-reply/category aggregation.
- Both session detail component tests pass in Chromium. The updated desktop
  screenshot was visually inspected; the existing narrow-layout check passes.
- Contracts build, server/web typecheck, server/web production builds pass.
- Final private assembly `44901b7`, pinned to public `bd63be30`, passes 1,119
  server cases with 38 explicit skips, 16 mock cases, production builds,
  artifact/input guards and browser credential-transport checks. Its build log
  is `/home/khing/.cache/parasaur-enterprise-current.ihts5hya/build.log`.

This source change is not yet deployed. It does not resolve provider quota
availability, historical readings, cross-node factory/chat reconciliation,
GitLab persistent auth, or memory soak acceptance. The prior live Codex/Mistral
token discrepancy is not attributed to cache without observing the raw counters
on a newly deployed adapter.
