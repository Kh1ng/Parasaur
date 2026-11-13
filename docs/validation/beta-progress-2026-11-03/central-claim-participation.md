# Central participation in shared claims

The live central identity was not a worker registration. That is intentional:
the registry refuses central's own endpoint to prevent recursive liveness
polling (#944). However, the claims routes required every identity to be a
registered worker. An isolated live acquire under central's identity returned
400 before this correction. Adding a self-polling registry entry was rejected
as a solution.

Public `02edaeb8` moves the three lease routes into one claims router. The
central identity must have the requested profile in its own configured profile
inventory. Every other identity still requires worker registration and the
declared profile. Missing or malformed local discovery returns 503 without
granting a lease; unknown profiles return 400. Store mutations remain
synchronous after authorization, so concurrent callers still arbitrate one
lease. Existing authentication, rate limiting, lease duration, owner checks
and worker-role restrictions remain in place.

## Validation and live deployment

The new regression fails against the original server. All 25 focused Linux
claim/store/node-role tests pass with the correction. The Windows claim/store
run passes 20 tests with one explicit platform skip; the broader Windows run
also hits an existing POSIX CLI fixture's status-read failure, which passes on
Linux. Server typecheck and build pass. Private assembly `efd5b51`, pinned to
`02edaeb8`, passes 1,115 server tests (38 explicit skips), 16 mock cases,
production builds and private artifact/input/credential-transport checks.
Log: `/home/khing/.cache/parasaur-enterprise-current.6r06rehm/build.log`.

Two compiled modules are deployed to central: the updated server and new
claims router. The server's exact preimage matches the earlier notification
backport. Legacy panel authentication, notifications, chat index, credentials
and node identity are preserved. Worker software did not need replacement.

A diagnostic work ID, unrelated to any real issue, was exercised from Windows
over TLS and from central. Each node acquired and renewed its own lease; the
other received 409 with the correct holder. A non-holder's release did not
remove that lease. After release, the other node could acquire it. All test
leases were released. No issue, review, PR or assignment was changed.

## Controller configuration

The running central controller also lacked `COORDINATOR_TOKEN`, and its config
had no shared-claims URL. Its existing private `gah-loop.env` now contains the
existing API credential, written through the native installer environment
helper. The profile config changes only `defaults.registry_central_url` to
the existing tailnet Parasaur origin. Provider, review, publishing and capacity
policies are unchanged.

The first configuration attempt rolled back when verification read the process
environment during its startup transition. After correcting that readiness
check and repeating the idle preflight, the retry passes. The controller is
PID 604933; the API is PID 601678. MemoryMax remains 1,572,864,000 bytes and
MemorySwapMax remains zero. The existing controller was restarted only after
an idle child-process boundary and empty live chat/job/claim checks. This is
not a new memory soak; the earlier hour-long observation describes its earlier
PID and unchanged binary.

The same live lease test passes using the new controller's actual environment
and configured tailnet URL. Its credential and identity match the API. Central
remains absent from the worker registry, preserving the self-polling guard.
TLS health reports healthy/provider-registry ready at 09:05 UTC on 2026-11-06.

Metadata-only records are in ignored `artifacts/central-live-20261105/`:
`central-claims-api-deployment.json`, `central-claims-client.json`,
`live-central-claims-02edaeb8.json`, `live-controller-claims-02edaeb8.json`
and `central-claims-post-health.json`. The reproducible artifact is
`artifacts/parasaur-claims-02edaeb8/`. Private config/environment backups stay
on central under `~/.local/state/gah/central-repair-20261105/`.

Subsequent [native reviewer readiness](native-review-readiness.md) transfers
matching Windows factory execution/review policy and verifies the native gate.
These lease
checks do not prove simultaneous real dispatch, full factory progression,
publication, provider billing reconciliation or the Mac memory report.
