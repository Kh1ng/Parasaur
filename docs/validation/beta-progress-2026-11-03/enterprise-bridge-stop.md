# Enterprise bridge stop boundary — 2026-11-04

The workspace execution trace found a current lifecycle defect: stopping the
outbound node bridge could still forward a late claim reply, and Stop did not
join detached HTTP work. A new regression reproduced a local request starting
after Stop. The repair rejects late claims, cancels pending RPCs and waits for
owned HTTP/delivery cleanup without publishing late frames.

The exact private runtime `711f70bf`, assembled against public core `bd377e21`,
passed **1,004 server tests**, one opt-in skip, no failures, plus **12 mock
tests**, typechecks/builds, artifact/input guards and transformed transport
checks. Focused races cover a delayed claim, delayed HTTP cancellation cleanup
and a pending network call. Disposable SQL/Auth/edge/PostgREST/relay integration
also passed; owned fixture services stopped afterward.

No frontend source changed from the preceding Windows two-account UI check.
This repair is private and undeployed; it does not change the public
`b0ea279f` recovery artifact or the installed native desktop/WSL CLI.

The source audit also confirms that portable workspace provider aliases,
shared-hardware execution isolation and staged local/cloud configuration
migration remain implementation work. Auth, membership and relay fixture passes
do not satisfy those requirements. They must not be described as only waiting
for hosted credentials or final acceptance. The full finishing goal is pending.
