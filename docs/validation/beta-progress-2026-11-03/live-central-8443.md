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
