# Native recovery when dashboard app files fail

The existing owned Windows preview exposed a blank root document at the saved
central origin. When the dashboard's JavaScript fails to load, the shared web
UI cannot render its connection controls. The native Settings menu remained
available, but the blank content offered no explanation or next action.

The native shell now injects a small recovery watchdog after a document from
the configured central origin finishes loading. If its `#root` still contains
no app content ten seconds later, it shows an explanation, the public origin,
Retry dashboard and Open This computer. It needs no dashboard script or CSS.
A slow app that renders during the grace period, an existing app error screen,
and documents without the dashboard root are left alone. The origin display
omits paths, query strings and fragments.

The new `gah://connection-recovery` navigation opens only the bundled local
screen and only when requested from the saved central origin. Scheme, host and
port must match. It grants no remote native commands and does not relax the
existing Settings-page or credential capability boundaries.

## Verification

- Four Playwright tests passed on Windows, covering 1000px and 390px layouts,
  grace-period behavior, origin redaction, keyboard focus, retry navigation,
  existing errors and non-dashboard documents. Both screenshot sizes were
  inspected in two bounded passes. The final screenshots are under the ignored
  `artifacts/dashboard-recovery-20261103/` directory.
- All 27 native Windows desktop unit tests passed, including the new recovery
  origin boundary and the existing capability, credential and navigation tests.
- The Windows debug executable built successfully. A copy ran in its own temp
  USERPROFILE/AppData directories with a loopback HTML fixture whose JavaScript
  request returned 503. The real webview exposed the recovery heading, origin,
  retry button and local settings link through accessibility after the grace
  period.
- Clicking the link through Windows automation failed with
  `coordinate input geometry is unavailable`. Refreshing the window and trying
  keyboard navigation did not establish activation. The actual native link
  click remains unverified; do not turn the successful rendering and unit
  boundary checks into a claim of completed native action acceptance.
- The bundled Settings browser regression passed. Scoped Rust formatting,
  workflow YAML parsing and whitespace checks passed. Whole-desktop formatting
  still reports pre-existing differences in `setup_check.rs`; those unrelated
  lines were not reformatted. Native compilation reported existing unused
  variable/import/field warnings outside this change.

The preview and its loopback server were stopped. The two earlier Parasaur
windows and the real saved central URL were untouched. The new code is local;
no installer or signed updater was published. MacBook and live-host work remain
explicitly deferred.

This handles a loaded dashboard document whose app never mounts. A navigation
that never finishes loading still relies on the native Settings menu or the
webview's own network-error UI; that broader navigation case is not accepted
by these tests.
