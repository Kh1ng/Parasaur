# Isolated host supervision

`gah isolation-host` is a local, private-descriptor protocol used by the enterprise
compute connector. It is not a remote CLI operation and does not authenticate
cloud jobs itself. The trusted connector must revalidate the exact work claim
before approval and every heartbeat. Do not expose this command to a dashboard,
forge payload or arbitrary command endpoint.

The host reserves requested memory (at least 512 MiB, including small-job
supervision overhead) through the existing node-capacity registry. The same
memory floor, CPU settings, PSI checks, flock format and stale-lease recovery are
used by local controller workers. The explicit owner-private TOML configuration
supplies that policy; ambient routing and provider settings are not loaded.
Controllers sharing capacity must use the same user/runtime registry.

The command establishes the resource scope in `parasaur-jobs.slice` and passes
a duplicate of the locked lease to Bubblewrap's outer monitor using `--sync-fd`.
The connector's trusted init must close all host descriptors and run as PID 1
with `--as-pid-1` before forking the job. Using Bubblewrap's default visible
reaper would expose the lease through `/proc/1/fd`. The private connector includes
the trusted reaper, affirmative approval gate and sealed filesystem setup.

The launcher child registers a kernel parent-death signal before exec, with
parent checks on both sides. Set-ID or file-capability launchers are refused.
The same locked open file description stays with the monitor if the broker is
killed abruptly. Normal broker cleanup retains its own lease until the launched
process is reaped and the owned cgroup is empty. Unknown cgroup state is not
treated as empty.

## Descriptor contract, version 1

| FD | Purpose |
| --- | --- |
| 0 | Connector commands: `h` confirms a freshly verified lease; `1` approves execution once; EOF or any other byte stops execution. |
| 1/2 | Bounded by the connector as job stdout/stderr diagnostics. |
| 3 | Private Bubblewrap options, NUL separated. |
| 4 | Bubblewrap's dedicated namespace/status stream. |
| 5 | Trusted `gah.isolation-host.v1` JSON-line receipts; never inherited by the job. |
| 6 | Host-owned locked lease passed to the outer monitor. |
| 7 | Private job environment data; mounted read-only and applied only after approval. |

The host reports `admitted`, `deferred`, `renewed` and `stopped` with the exact
scope unit. Stopped receipts carry the observed launcher exit code/signal and
stop reason. Capacity denial is a deferral, not successful execution. No backend
outcome or usage measurement can be inferred from admission alone.

The heartbeat deadline defaults to ten seconds and is bounded to two through
fifteen seconds. A frozen Node owner cannot renew it. Expiry, owner EOF, rejected
protocol input, shutdown or the job deadline stops the owned cgroup. The host
uses the kernel `cgroup.kill` path as well as killing its launcher, independently
of Node or D-Bus responsiveness. If cleanup cannot be verified, it retains the
lease and keeps trying instead of dropping capacity or returning success.

The native host protocol bypasses normal CLI alias repair and provider refresh.
It does not open a provider login, infer an account, or fall back to an unsandboxed
command. The command requires Linux, a working user systemd manager and delegated
cgroup v2. The caller supplies a matching host binary and trusted Bubblewrap.

The automatic cloud consumer, durable execution journal and production
dispatch/client runtime remain separate integration work. This protocol does not
make a queued cloud work item execute by itself.
