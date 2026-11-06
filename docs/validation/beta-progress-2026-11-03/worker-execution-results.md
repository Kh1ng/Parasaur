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
`checks.log`). The full private assembly and reproducible deployed-runtime
backports are verified separately before rollout. No controller restart is
required for this server correction; its ten-hour observer remains independent.
