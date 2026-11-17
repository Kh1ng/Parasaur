# Recovery from unfinished desktop navigation

The previous recovery script ran only after load finished. A server that never
sent headers, or a blocking app asset that never completed, could bypass it.
Settings also awaited setup, role, keychain and update reads while disabling
connection controls, preventing escape when one of those reads stalled.

The native shell now retains one navigation attempt and checks it with one
application timer. After 15 seconds it probes the configured central document
for content, allowing two more seconds for content to appear. An unacknowledged
attempt returns to bundled Settings with a focused timeout notice. A newer
navigation or local Settings invalidates queued recovery. The probe's observer
expires; no navigation history or per-navigation native tasks accumulate.

Content acknowledgement uses one read-only command, scoped to the dashboard
window and checked against the saved central origin and active attempt. It
cannot read credentials, run processes or operate local settings. Same-document
routes and gateway forms remain usable. This detects presentation, not server
health or factory progress; an existing error screen counts as content.

Native validation found that `http://*` and `https://*` capability patterns
exclude explicit ports. This rejected acknowledgement at loopback ports and
would reject the saved central's `:8443`. The remote bridge patterns now permit
ports; their existing configured-origin and argument checks remain in place.
A regression exercises Tauri's actual URL matcher for every remote capability,
including default ports, HTTPS 8443, HTTP 3773 and rejection of file URLs.

Settings loads saved connection details first, then runs read-only status
checks independently. Setup/role checks disable only their own controls.
Overlapping actions cannot prematurely enable a still-busy setup control.

Validation on Windows:

- 34 native tests and the debug build passed. The explicit-port regression
  failed with the previous capability patterns before passing with the fix.
- Seven browser recovery/presentation tests passed, including late content,
  expired observers, SPA navigation and embedded-document rejection.
- The Settings script passed its existing flows plus four held status reads:
  setup, role, owner credential and updater. Each left Save and connect usable.
  Desktop typechecking passed.
- Actual isolated native windows returned to Settings with the notice for a
  server that never sent headers and a blank document blocked on an unfinished
  script. A populated document remained open in a fresh isolated launch, and
  another document populated at 16 seconds remained open during the grace.
- One concurrently launched populated fixture exposed `about:blank` rather
  than its document; the fresh isolated confirmation succeeded. This anomaly
  is not accepted as a successful startup and remains a limitation of this run.
- Settings screenshots at 1000px and 390px were inspected; notice and connection
  controls fit without horizontal overflow.

These are local client checks. MacBook RSS, the deployed central's assets,
live review gates, signed update delivery and physical iOS remain unverified.
MacBook work is deferred at the owner's request.

## Optimized Windows installation

The optimized NSIS/MSI build passed for source `b280a5e2`. A desktop-only
candidate was staged under ignored `artifacts/parasaur-desktop-b280a5e2`, with
its exact revision and hashes in `desktop-artifact.json`. This is separate
from the older combined desktop/worker candidate; their manifests must not be
mixed. The new NSIS SHA-256 is
`6d55cf0e988d3eb35026b93d93e5a0cd379b16423695ce23c1dbe6fdc6dff468`.

The current-user installer upgraded the existing `ParasaurNativeTest` app and
returned zero. The previous executable was backed up outside the repository.
The installed executable matches the built bytes after only Tauri's expected
`UNK` to `NSS` bundle marker patch, with installed SHA-256
`9a86122c366fe3fb5d950f2b5785a0e1395150a9dd8f794d9da76e1ebf522f82`.
Saved desktop settings were byte-for-byte unchanged. The updated app was
started by its exact installed path. No worker/service setup, administrator
prompt, signed update publication or live backend change was performed.

The actual installed window returned to local Settings with the timeout notice
for its saved central connection; the native Settings menu remained present.
The settings hash was still unchanged after launch. The updated app was left
open, and all owned navigation fixture apps and their loopback server stopped.
