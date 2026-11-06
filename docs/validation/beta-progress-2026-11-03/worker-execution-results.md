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
