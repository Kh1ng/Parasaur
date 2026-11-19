# Windows worker recovery — 2026-11-05

The existing WSL worker was version 0.1.2 and still configured to use
`https://hermesagent.tail82695.ts.net` (Hermes). Parasaur is now on the same
tailnet host at port 8443. The worker's Windows logon task was Ready rather
than Running; WSL stopped between command sessions despite its enabled user
service. Restarting the existing task restored its foreground keepalive and
refreshed Windows-to-WSL forwarding.

## Installed recovery

The config edit changes only `defaults.registry_central_url` to the HTTPS
8443 origin. The original config is retained at
`~/.local/state/gah/worker-recovery-20261105/config.toml.before`; profiles,
role and all other parsed fields are unchanged. The initial CLI-based edit
normalized additional defaults; those expansions were removed before applying
the final minimal URL edit.

A staged public runtime from `6b101b98` with optimized CLI `108c9f5` passed
its separate loopback health and authenticated API probe, including worker
role and the correct central origin. The probe was stopped before installation.
The runtime was installed only after checking for active worker agent processes.
No active agent was interrupted.

The upgraded user service reports 0.3.0, retains node ID
`011f5dca-4228-4aff-a7f0-d12953d86c0a`, and uses the same credential,
config and advertised address. New release:
`~/.local/share/gah/worker/release.beta108c9f5.o7_ebsik`.
CLI SHA-256:
`ec60c6b835ae033bdd9798b54c92828d33aa03d5cb0a98b05c393af30d7ad2b6`.
Rollback of the unit, private environment, identity and launcher/register files
is retained in `~/.local/state/gah/worker-recovery-20261105/worker-file-rollback/`.
The temporary credential-bearing installer settings file was deleted after use.

The worker was re-registered for profile `gah` against the HTTPS 8443 central.
Windows and the central VM both read its healthy worker-role endpoint through
`http://100.98.69.105:3774`. This retains the existing trusted-tailnet HTTP
transport and central-only Windows firewall rule. No listener was added.
Central's factory check now reads worker loop and status data; the last-decision
read remained unavailable, and the returned status had an older generation time.
These results establish connectivity and compatible runtime, not successful
factory execution or complete fresh factory diagnostics.

## Launcher retry regression

The installed launcher ended after `wsl.exe ... sleep infinity`. An interrupted
WSL process was therefore reported to Task Scheduler as a successful script
completion. Its configured three retries were not applied, leaving the task
Ready and WSL without its keepalive after a distro restart.

Core `d47ad2fc` makes that unexpected return fail the launcher. Two Windows
PowerShell tests execute the actual generated launcher with mocked native
commands: both a zero and nonzero keepalive exit fail on the old implementation
and pass on the fix. The complete Windows installer parses without syntax errors.
No real service, WSL process or forwarding rule is changed by those tests.

The installed script lives under `C:\Program Files\GAH Worker Startup` and
requires administrator permission to update. A guarded application script is
staged at `artifacts/apply-worker-supervisor-admin.ps1`; it checks the exact
installed-file hash, retains rollback, appends the tested failure guard, checks
PowerShell syntax and restarts only the existing worker launcher task. The
administrator application completed successfully. The installed script hash is
`1835406bf4add934b5fe2617c955e3db46e96cc0c7361a00ad36b300518ff64b`;
rollback is retained under
`C:\Users\cspur\AppData\Local\ParasaurRecovery\worker-startup-20261105`.
The launcher task is Running and the worker retained PID 2548 and its identity
through the launcher restart. Windows still reads healthy version 0.3.0 afterward.
A future forced WSL termination was not used as a live test; retry behavior is
covered by the two real-script PowerShell regression cases.

## Remaining pipeline findings

The central's exact review branch is present and bounded `ls-remote` succeeds
with the controller's noninteractive Git environment. A prior full branch fetch
still timed out at 300 seconds and triggered infrastructure backoff; its cause
is not yet established. Submodule recursion was checked and the enterprise
submodule is not initialized on that checkout. No Git remote or fetch policy
was changed without a reproduced cause.

The live Factory intake also shows issues assigned to `Ram-Rod6198` excluded
as managed by another loop. Assignments were not changed. Central node capacity
continues to defer review launches. These are separate from worker reachability.

The later [execution-capacity check](execution-capacity.md) fixes the worker
role guard's missing GET last-decision permission. The direct endpoint and
central relay now read successfully, with POST still rejected. Cold status
latency and actual provider/worker execution remain acceptance work.
