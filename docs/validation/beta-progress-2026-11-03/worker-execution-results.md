# Worker execution ownership and results

This correction is a prerequisite for native remote handoff. It does not yet
move native controller decisions or their authoritative ledger to a worker.

The regression first failed against the previous implementation: cancellation
returned an error and released its profile slot while its execution promise
remained pending. A second issue let cancellation of the newest queued session
erase its live predecessor's queue dependency. Both cases now retain ordering
and ownership until completion or a confirmed cancellation.

An unconfirmed cancellation remains `stopping`, has no end time or execution
result, stays in the active roster and publishes a status update. Actual later
completion releases its slot and records the observed result. A closed POSIX
parent does not release the profile while its process group can still execute.
Linux zombies do not count as executing; observation errors fail closed. Native
Windows does not advertise this capability; the Windows worker uses WSL.

Terminal sessions carry `execution` with an explicit reason, nullable observed
exit code and nullable signal. Transport sentinel -1 never becomes an exit code
or successful zero. Cancellation before launch is distinct from a launched
process. A zero CLI exit is process evidence, not proof that a job published or
merged successfully. Native handoff still needs its ledger and artifact result.

Fleet ownership is persisted as an offered receipt before launch. A lost start
acknowledgement remains reconcilable by request id plus exact work identity.
Disconnects and missing worker rosters do not invent terminal results or expire
ownership. Late acknowledgements can recover an offered receipt. Unrelated
worker broadcasts cannot replace the owned session. New remote dispatches
require `dispatch_execution_results`; missing or malformed fresh terminal
results remain uncertain. Historical terminal receipts retain their original
status and show missing old exit details explicitly.

The UI names unconfirmed execution and displays actual exit/signal information.
It does not label a missing exit result successful. Existing unknown receipts
need process/ledger evidence from their worker before they can be retired;
retrying with a different request id does not create a second copy of that work.

## Verification

The negative control failed on the expected premature terminal state. The
focused Linux server/CLI/fleet/WebSocket run passes 96 cases, including real
process-group cancellation, parent exit with a surviving child, queue ordering,
lost acknowledgement across coordinator restart, wrong-work correlation,
disconnects and absent/malformed exit results. Two Windows rendered component
cases and web typechecking pass. Desktop and 390px screenshots were inspected.

Source and logs are retained under
`/home/khing/.cache/parasaur-worker-handoff.ruxtnphq/` (`negative.log` and
`checks.log`). Private assembly `1ee0eff`, pinning public `b27bdf55`, passes
1,155 server cases (38 explicit skips), 17 mock cases, production server/web
builds and private artifact, input, credential and transport guards. Its log is
`/home/khing/.cache/parasaur-enterprise-current.qlf_yk5r/build.log`.

## Live deployment and acceptance, November 6

The targeted builds reproduce every installed preimage before applying the
correction. Seven runtime modules changed on each host; protected legacy
handoff, chat lifecycle, claims, activity and recorded-usage modules retain
their installed hashes. Per-module manifests and archives are retained under
ignored `artifacts/parasaur-worker-results-b27bdf55/`.

The idle Windows WSL API updated from PID 230925 to 339737 at 15:02:52 UTC,
preserving node identity, configuration, credential and CLI hashes. Central
updated from PID 756927 to 826870. Its first post-install metadata observation
timed out; read-only verification subsequently passed against the same PID at
15:11:30 UTC. There was no restart in response to that timeout. Central owner
authentication and its 96 observed chat sessions remain available. The installer
does not modify configuration or service environment; an exact pre/post central
configuration hash was not retained across the pending verification, so that
stronger comparison is not claimed. Runtime/protected-module and CLI hash
checks passed.

Shared web `b27bdf55` deployed at 15:15:03 UTC. All 30 served asset files,
including 29 JavaScript chunks, match the immutable build manifest. Public HTML
bootstrap routing configuration and old immutable assets were retained. This
web deployment did not restart the API or change the CLI.

Owned dry-run `factory-dry-run-074cd6d87be540f59dfa118dbd44db77` selected
the configured OpenCode/Mistral route on pinned worker
`011f5dca-4228-4aff-a7f0-d12953d86c0a`, fix mode, a diagnostic branch and no
real issue. It did not request a manual-worker bypass. Central returned HTTP
200, terminal lease and stopped status with observed execution
`{reason: exited, exitCode: 0, signal: null}` without timing out. The worker
independently reported the same result and zero active sessions afterward.
This validates transport and process evidence, not a published factory job.

Controller PID 666974 and memory observer PID 667504 were unchanged. At
15:16:05 UTC, the existing observer was live with 259 samples spanning
15,764.946 seconds, zero observation gaps and zero cgroup OOM kills. Latest
controller RSS was 558,084 KiB. The ten-hour observation and sustained agent
load acceptance remain incomplete; native automatic factory handoff remains
unimplemented. The selected CLI and the controller's older running inode were
not replaced or restarted by this rollout.

Metadata-only deployment, dry-run and memory reports are retained under ignored
`artifacts/central-live-20261105/`. No issue was closed, no source was pushed,
and no mobile or Mac acceptance was performed.

## Request binding follow-up

Native handoff inspection found another gap: pending and saved starts reused a
request id without checking its complete execution request. The work key also
intentionally omits execution controls, so joining active work could return a
preview receipt to a caller requesting real execution, or the reverse.

Worker starts now bind request ids to a canonical fingerprint of profile,
provider/account, repository/work target, backend/model, budget, execution
flags, reasoning and GitLab context. Central additionally binds the requested
node and upstream coordinator. Pending joins and saved receipts require the
same binding, including across central restart. Operational receipt retention
can change without changing execution. A changed request is refused before
launch; a historical lease without the binding remains inspectable but cannot
prove an equivalent replay. Reconciliation and cancellation of its existing
session remain available. Session cleanup and unpinned receipt retention keep
their existing limits; worker restart persistence is not newly promised.

The fingerprint is separate from the work key: the latter still prevents
concurrent duplicate work, while the fingerprint determines whether the caller
can receive that work's existing receipt. Snapshotting the request before
asynchronous admission also prevents caller mutation from changing its meaning.
Worker start publication occurs only after the pending request owns its slot,
so a synchronous start subscriber cannot recursively launch a duplicate.

Both new replay regressions fail against the previous implementation with the
expected missing rejection. The corrected server builds and passes all 99
focused session/CLI/fleet/WebSocket cases. These include pending/terminal
replays, changed work and controls, active-work joins, central restart, legacy
receipt inspection and reentrant start publication. Logs are
`request-binding-negative.log`, `request-binding-build.log` and
`request-binding-checks.log` under the same isolated source parent above.
Public `5328f873` is deployed as a targeted API backport on both hosts. Private
`d3eca7c` pins that revision and passes 1,158 server cases (38 explicit skips),
17 mock cases, production builds and private packaging/input/credential and
transport guards. Its log is
`/home/khing/.cache/parasaur-enterprise-current.qxgrw5g3/build.log`.
The targeted builds reproduce both installed module preimages, verify that the
new identity module was absent, and pass the session/fleet/real-process boundary
checks. Manifests, archives and source patch are retained under ignored
`artifacts/parasaur-request-binding-5328f873/`; build log:
`/home/khing/.cache/parasaur-request-binding.w8vdcv92/build.log`.

Worker API PID changed from 339737 to 352938 at 15:28:20 UTC. Central changed
from 826870 to 841661. Its first post-install metadata observation timed out;
read-only verification of that same PID passed at 15:30:26 UTC. Stored preflight
hashes now permit exact configuration, owner credential, CLI and chat-index
comparisons across a pending verification; all passed. The 96 observed central
chat sessions, protected runtime modules, controller PID 666974 and observer
PID 667504 were preserved. There was no restart on observation timeout. Shared
web remains `b27bdf55`; no UI change is needed for this server correction.

Owned request `factory-dry-run-04831e3ad71941f89b1aaf3166e8c483` completed on
the pinned Windows WSL worker with observed exit code zero. Both central and
the worker independently refused the same request with `dryRun` changed to
false, and both returned the original session and execution result for an
unchanged replay afterward. The HTTP route currently reports those conflicts
as 502 with the explicit mismatch message; the WebSocket/internal error has
code `DISPATCH_REQUEST_CONFLICT`. No real issue or manual-worker bypass was
selected. The final independent worker roster was empty of active sessions.

At 15:31:07 UTC the original observer was still live: 274 samples over
16,665.025 seconds, no gaps and zero cgroup OOM kills. This is not the ten-hour
or sustained-agent-load acceptance. Metadata reports are retained under ignored
`artifacts/central-live-20261105/request-binding-*.json`. Native policy/history
transfer remains unimplemented; this correction does not establish complete
factory execution or close an issue.
