# Live central discovery and dashboard failure

Read-only HTTPS checks on 2026-11-03 confirmed the actual Parasaur service at
`https://hermesagent.tail82695.ts.net:8443`. The original HTTPS listener still
returns 404 for `/api/info`; its previously identified service is Hermes.

| Request on port 8443 | Observed result |
| --- | --- |
| `GET /health` | 200; role `central`, version `0.2.0`, status `healthy`; Rust backend, provider registry, WebSocket and node-role readiness true. These checks do not prove dashboard rendering or factory progress. |
| `GET /api/info`, without a credential | 401; remote access requires authentication. |
| `GET /` | 200 HTML with title `Parasaur`. |
| `GET /assets/index-oGMXkd18.js` | 500; fixed generic JSON server error, despite a JavaScript content type. |
| `GET /assets/index-DwFxA1zN.css` | 500; the same fixed error, despite a CSS content type. |
| `HEAD /assets/index-oGMXkd18.js` | 200, length 487342, last modified 2026-10-26. |
| Ranged JS GET, bytes 0–31, identity encoding | 500, while retaining the expected content-range metadata. |
| `HEAD /assets/parasaur-missing-probe.js`, Accept JavaScript | 404. |

The document therefore references assets whose body delivery fails after file
metadata is available. The exact filesystem/streaming cause is unverified;
inspect the deployed process, build ownership/read permissions and service
logs. Do not treat a ready health response or successful asset HEAD as a
successful UI check. The current implementation's generic error response
does not establish the underlying error code.

Windows's real saved `central_url` and `remote_central_url` now use the confirmed
port, preserving the other settings and a timestamped local backup. The owned
native test window was also saved/connected through its Connection controls.
Accessibility verified its document URL on port 8443; no app content rendered.
Monitor screenshot capture failed during navigation, so this is navigation and
HTTP evidence, not a successful native rendering check. The isolated sample
backend remains available separately; it is not the live central.

No central token was found in the known Windows or WSL configuration. The
native connection panel also reported no saved owner token for this origin.
A private credential card requests the central's `COORDINATOR_TOKEN` for
read-only factory/controller checks. The earlier MacBook SSH password requests
expired; no SSH session or host configuration change succeeded. No token,
pairing, role, service, migration, review gate, or runtime job was changed on
the live host.

Follow-up checks from Windows returned connection timeouts for health, API
info and the previously referenced JavaScript asset (12-second curl limit).
Tailscale reported the host online, which does not establish HTTP availability.
The 500 responses above are the earlier successful network observations, not
a claim that the current requests reached the service. The owner then deferred
MacBook work; the new private SSH credential request was declined. No request
remains pending and no host changes were made.

## Windows recheck and diagnostic improvement, 2026-11-04

Fresh read-only requests reached the service again: health and dashboard HTML
returned 200, `/api/info` returned 401 without credentials, and both assets
referenced by that HTML still returned 500 with the fixed generic error body.
The live asset failure therefore persists; earlier timeout evidence did not
establish recovery.

The local server now logs an allowlisted system error code and operation plus
a fixed request category (`dashboard_asset`, `api`, or `document_or_other`).
It continues to omit raw messages, stacks, filenames, query strings and request
contents. Unknown codes are recorded as `UNKNOWN`, not copied from the error.
This makes an asset permission failure distinguishable from I/O, missing-file,
disk-full or descriptor exhaustion without exposing private data.

A Windows HTTP regression injects a stream-open failure after successful file
metadata lookup. HEAD returns 200 without opening a stream, GET returns the
unchanged generic 500, and the operational log records only `dashboard_asset`,
`EACCES`, and `open`. This reproduces the failure shape, not its live cause.
All four request-error tests, 14 document-router/web-root tests, and server
typechecking passed. These tests use no provider executions or credentials.

This change has not been deployed to the live service. On an updated host,
inspect `journalctl -u gah-server` for the fixed request-failure record after a
failed asset GET. `EACCES`/`EPERM` points to access checks, `ENOENT` to missing
files, and `EIO`/`ENOSPC`/`EROFS` to storage errors; use the host's actual service
configuration and filesystem evidence before changing ownership or rebuilding.
No authenticated pipeline evidence or live repair is claimed.
