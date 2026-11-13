# Provider initialization without hiding the API

The entrypoint awaited `refreshAllFromGah()` before calling `server.listen()`.
That refresh awaits a light CLI status snapshot, which still reads the ledger
and availability. A slow query therefore prevented even health and repair
requests from connecting. The earlier notification rollout exposed this
startup barrier in the live process's lifecycle trace.

Public `579a3c9b`, with merge-context cleanup `e362380f`, starts the refresh
after the HTTP listener opens. ProviderRegistry owns its readiness check for
all refresh callers: pending remains false, a failed read records a fixed
error, and a successful later read clears that error and marks the check ready.
The existing fallback provider behavior is retained. Listener availability
does not imply fresh provider authentication or quota data. `/health` reports
starting until all existing checks are ready; no readiness result is fabricated.

The alternative of simply extending startup's observation deadline would
retain the API-wide dependency on a potentially slow ledger query. Moving the
refresh removes that dependency and keeps readiness evidence in its owner.

## Verification

The new real-entrypoint test uses an isolated home/config, an executable fake
CLI and a filesystem gate. It holds status indefinitely, verifies health and
authenticated info answer while readiness is pending, then injects a failed
read and verifies explicit failed readiness. A provider refresh subsequently
recovers to healthy. All fixture processes are terminated as one owned process
group and removed; no real provider, credentials or external service is used.

The test fails at the intended listener assertion against the original order
and passes with the correction. Both outcomes are retained in ignored WSL
`parasaur-startup-check.n7_dxz87` and `parasaur-startup-check.osgz2pfp` directories.
The notification-compatible public backport passes 34 Linux tests, including
the entrypoint, migration, preferences and real WebSocket boundary. Server
typecheck passes.

Private assembly `67a06e0` pins `e362380f` and passes 1,110 server tests
(38 explicit skips), 16 mock cases, server/web production builds/typechecks,
private input/artifact guards and credential transport checks. Its log is
`/home/khing/.cache/parasaur-enterprise-current.s7n30l0z/build.log`.
The initial overlay merge caught the old awaited initialization and then a
blank-line context mismatch. Both were resolved explicitly; enterprise cloud
startup hooks remain in the overlay. The assembled entrypoint regression passes.

## Live deployment

The API remains public base `6b101b98` with the earlier notification/protocol
backport through `0ce6e7fb`, plus the startup correction through `e362380f`.
Only compiled `bin.js` and `provider/ProviderRegistry.js` were replaced for
this rollout. Their base hashes and the installed managed runtime lock match
the reproducible build. The source patch, artifact hashes and manifest are in
ignored `artifacts/parasaur-startup-e362380f/`. Shared web remains `0ce6e7fb`.
The staged persistent-auth API was not installed.

A fresh preflight checked 100 chats and an explicit empty active-job roster.
Deployment passed at 2026-11-06T06:59:28Z, leaving API PID 530676 and controller
PID 437234. The first observed info response arrived 6.362 seconds after start;
the health body explicitly reported providerRegistry not ready and status
starting. A subsequent local and Tailnet health read reports healthy with every
check ready, and the provider refresh success is present in the service log.
This is one live restart, not a startup-latency benchmark or repeated soak.

The chat index, node identity, configuration, owner credential, legacy handoff
bytes and all 261 real notifications in the verification window were preserved.
The controller was not restarted and no memory guard was changed. Code and
activity backups remain under the central repair directory. Metadata-only
reports `provider-startup-api-deployment.json` and
`provider-startup-readiness.json` are copied into ignored
`artifacts/central-live-20261105/`.

The WebSocket welcome path separately awaited a full status snapshot before
sending its handshake. [Welcome correction](websocket-welcome.md) subsequently
removes that barrier while retaining missing/empty distinctions. Physical
capacity, complete factory progression, real GitLab sign-in and Mac memory
acceptance also remain open. Rebuild the old staged auth package from current
source before its eventual rollout, retaining both server corrections.
