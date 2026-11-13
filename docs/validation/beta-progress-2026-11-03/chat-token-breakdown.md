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

The central API module and shared web are deployed from `bd63be30`. The adapter's
compiled preimage exactly matched the installed archive-lifecycle fix. An idle
preflight checked 100 sessions with no active chats/jobs before the API restart.
API PID 628728 serves the updated module; controller PID 604933, node identity,
owner/config/CLI hashes, chat index and 268 real notifications are preserved.
All 30 served JavaScript chunks match the immutable public build. Initial
provider discovery waited on disk I/O, then completed: `/health` reports healthy
with providerRegistry ready. Backups and metadata-only reports are under the
existing central repair state directory.

Windows worker PID 178252 now runs the same adapter module. Its restart
preflight confirmed no active sessions, controller, running workers or claims.
Configuration, credential and node identity are preserved. Two disposable
live worker chats then completed without tools/permission requests and were
archived:

- Codex `gpt-6-astra`: input 3,236, output 10, cache read 12,032, total 15,278;
  reasoning is explicitly zero, cache write and cost remain unknown.
- OpenCode `tak-mistral-vibe/zai-glm-5-3`: input 6,315, output 81, cache read
  2,240, total 8,636; reasoning, cache write and cost remain unknown.

In these samples input + output + cache read equals the independently reported
total. This explains the analogous missing category without imposing an additive
identity on other providers. The live records are ignored
`artifacts/central-live-20261105/chat-usage-live-smoke.json` and
`chat-usage-mistral-live-smoke.json`; no private prompt/config/credential content
is retained in those metadata reports.

This does not resolve provider quota
availability, historical readings, cross-node factory/chat reconciliation,
GitLab persistent auth, or memory soak acceptance. The prior live Codex/Mistral
historical token discrepancies cannot be reconstructed from records that
discarded those categories. The new samples establish the fresh readings only.
