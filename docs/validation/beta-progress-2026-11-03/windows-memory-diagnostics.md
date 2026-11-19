# Native Windows memory diagnostics — 2026-11-05

Public source `8ed537337510f97d8e00cd2c11d27780626dd963`, private pin
`bc7d050c0fd03a24410cf13342c1b3e5179d7761`.

The existing diagnostics sampler invoked Unix `ps` on Windows and silently
returned no processes. A native check reproduced this: the sample did not even
contain the sampling Node process. That prevented useful host-process attribution
in a Windows diagnostics export.

The Windows sampler now queries Win32_Process through a hidden, noninteractive
PowerShell child. It requests and exports only process ID, parent ID, image name
and working-set bytes, converting the latter to the existing KiB field. It does
not request command lines. Output is limited to 8 MiB and execution to five
seconds. Linux/macOS retain the existing `ps` path. Both paths share selection
of the largest processes plus every server descendant.

Windows CPU percentage and Unix load average are unavailable and exported as
`null`. `processes_status` explicitly distinguishes available collection from
an unavailable collector; historical samples without that field remain unknown.
Host and server memory remain measured when collection fails. The existing
rotated timeline and owner-protected diagnostics export are retained.

## Verification

All **six focused diagnostics cases passed natively on Windows**, including
real CIM collection of the sampling process, largest/descendant selection,
bytes-to-KiB conversion, omission of command-line fields, missing-collector
fallback, timeline compatibility and export rate limiting. Server TypeScript
typechecking passed. The missing-collector case uses an isolated child with an
empty PATH; it retains measured server RSS and reports process data unavailable.

`artifacts/windows-process-diagnostics-tests.log` retains the native test output.
`artifacts/windows-process-diagnostics-sample.json` retains a metadata-only
sample: process collection is available and the Node process has a nonzero
working set, with CPU/load unknown. It is a point sample, not a memory soak.

The final assembly at
`/home/khing/.cache/parasaur-enterprise-current.fv_bon5q/source` passed
**1,095 server cases**, 38 explicit skips, and **15 mock cases**, plus
typechecks/builds and artifact/transport guards. Its sibling `build.log` records
the run. Native Windows checks pass on that final source. A direct sample of
the built Linux module also retained the sampling process and measured numeric
CPU/load values; `artifacts/linux-process-diagnostics-sample.json` records that
metadata-only check.

No Rust, Cargo, desktop production or private implementation source changed in
this increment. Their earlier isolation/native/Auth checks remain evidence for
their recorded revisions. The installed desktop and WSL CLI were not changed
again, and no live host or provider job was modified.

Windows host sampling cannot identify individual processes inside WSL guests;
those require sampling inside the guest. The Linux central's #1681 allocation
source and long-running stability remain unverified. MacBook work stays deferred.
This fixes missing Windows observations, not the live memory leak or its service.
