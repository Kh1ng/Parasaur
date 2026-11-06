# Execution capacity and worker decision diagnostics

The event-query hypothesis was measured before editing. At
2026-11-06T07:21:33Z the journal was 210,860,894 bytes, but the requested
24-hour gah window contained 3,194 records and produced 1,915,671 JSON bytes.
The streaming diagnostic discarded event contents; the CLI's measured peak
RSS was 16,488 KiB. The API process was using 88,368 KiB RSS. This snapshot
does not support rewriting the current query as the primary memory fix.
No event retention or query semantics were changed.

The central node reports 4 GiB total memory. Its configured implementation
reservation alone is 4,096 MiB, before the existing 2,048 MiB minimum safety
floor. That job cannot fit this node even with no other workload. Reviews use
the existing smaller reservation, but current available memory also falls
below their reservation plus the safety floor. No guard was lowered, and the
unrelated Hermes services were not stopped. Controller and API memory are not
the same measurement as total node headroom.

The registered Windows/WSL worker is reachable and healthy for profile gah,
with substantial available memory and no observed claims or workers. Its login
observations are separate gates: Claude reports missing login, Codex is not
configured, and GitLab reports an expired login. OpenCode's LMStudio auth
probe reports OK; model availability and real execution were not verified.
GitHub's login probe reports OK. These observations do not establish that
factory routing can safely execute the pending review on this node.

## Confirmed diagnostic defect

The central factory detail read calls `/api/loop/last-decision` on the worker,
but the worker route guard omitted that endpoint from its read-only allowlist.
A direct authenticated worker request returned 409. The central view therefore
reported its decision read unavailable/transport despite the node being healthy.

Public `5841d3a7` allows GET for this existing endpoint. POST remains rejected,
and the surrounding shared authentication boundary is unchanged. Ten Linux
node-role/factory tests pass, including the read/mutation distinction and the
existing worker restrictions. Server typecheck passes. Private assembly
`b580539`, pinned to `5841d3a7`, passes 1,111 server tests (38 explicit skips),
16 mock cases, production builds/typechecks and private input/artifact/transport
checks. Its log is
`/home/khing/.cache/parasaur-enterprise-current.y1vsexng/build.log`.

## Live worker correction

The update is a single compiled `nodeRole.js` backport to the Windows worker's
existing public `6b101b98` runtime. Its preimage matches the reproducible build
exactly. No dependency install, central service change, controller start or
provider login ran. Artifact/source hashes are in ignored
`artifacts/parasaur-worker-route-5841d3a7/`.

Before restart, the worker reported a stopped loop, a fresh empty runtime
roster and claim list, and an explicitly observed empty WebSocket session list.
Deployment passed at 2026-11-06T07:36:24Z. Worker PID changed from 359 to 112625;
node ID `011f5dca-4228-4aff-a7f0-d12953d86c0a`, configuration, credential and
worker role were preserved. GET last-decision now returns 200; POST still
returns 409. The original module and report are backed up under
`~/.local/state/gah/worker-recovery-20261105/decision-route-5841d3a7/`.

Central's decision read now reports available. A cold full-status read still
exceeded the existing five-second diagnostic deadline; the later warm read
reported status, loop and decision all available. Both results are preserved.
This does not claim that cold status latency, login setup or actual factory
execution is fixed. The worker currently has no recorded decision in its
24-hour window; that is not proof of idle or completed work.

Metadata-only evidence is in local ignored `artifacts/central-live-20261105/`:
`event-query-baseline.json`, `central-node-capacity.json`,
`windows-worker-capacity.json`, `execution-fleet.json`,
`worker-decision-route-deployment.json` and the before/cold/warm
`worker-factory-read-*.json` reports. The pre-existing unreachable Mac worker
registration was not removed. Its client-memory acceptance remains open.

Next execution checks must establish an exact usable worker provider/model,
preserve central claims and review policy, and verify actual work under the
worker's normal capacity guard. The central's local-only capacity refusal and
Michael's assigned issues must not be bypassed by changing ownership or limits.
