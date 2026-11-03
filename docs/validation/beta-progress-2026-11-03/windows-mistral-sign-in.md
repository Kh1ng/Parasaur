# Windows Mistral browser sign-in path

Windows previously rejected native Mistral sign-in before opening a window.
It now uses the existing native provider window and the selected WSL node's
installed CLI for verification and persistence. Unix verification behavior is
retained; window startup runs on the blocking worker pool so WSL preflight does
not block the native command thread.

Default connections retain their dedicated browser store. Each named connection
uses its own incognito WebView2 directory. The production browser-store helper
is also used by the native cookie-isolation test. Provider windows receive no
native command or event capability. Only the fixed authenticated Mistral usage
origin/path triggers automatic checking, and cookie filtering remains native.

The Windows helper sends the cookie through private stdin into WSL. A bundled
Python standard-library script creates an owner-only staging directory and 0600
cookie file there. It invokes the existing dashboard collector with an isolated
quota store, then requires a successful Mistral account observation before
saving. Named saves update only their selected credential; default saves replace
the default cookie and import the verified observation. The default preflight
refuses a custom cookie source in the effective WSL command environment.

Collector failure, refusal, invalid observations and save failure preserve the
previous connection. A subsequent usage/import failure is reported separately
after saving the verified session. CLI diagnostics are discarded and only fixed
result codes cross back to the Settings UI. Cookie values do not enter argv,
Windows temporary files or replies. The Linux process-group deadline is 195
seconds, covering two existing 95-second checks; the Windows transport adds
three seconds and at most one second to reap it. SIGTERM unwinds staging cleanup.

## Checks

- Windows native suite: 35 passed; six opt-in/child fixtures skipped by default.
- Actual Windows-to-WSL staging fixture passed with a synthetic CLI: default
  and named success; refused, failed, invalid and oversized observations;
  save failure; later usage/import failures; custom-source and sibling
  preservation; literal display labels; invalid cookie headers; missing execute
  permission; and interrupted collector cleanup. No provider request occurred.
- Actual native WebView2 fixture passed using the production store helper:
  two same-origin windows retained distinct HttpOnly session cookies. Only owned
  loopback pages were opened. Fixture windows/server closed; Windows retained
  some synthetic browser-cache files during teardown (OS error 32), so complete
  temporary-profile removal is not claimed.
- Read-only Mistral preflight against the actual saved Windows settings and
  installed WSL CLI passed. No real sign-in page was opened.
- The existing real WSL two-account execution/revocation fixture and production
  account-metadata read passed again after sharing the WSL process wrapper.
- Desktop Settings browser checks passed. Unix native tests were not rerun:
  WSL lacks GTK build prerequisites and MacBook work remains deferred.

Opt-in test filters are
`mistral_login::windows::tests::windows_staging_preserves_connections_until_verification`,
`mistral_login::windows::tests::installed_wsl_mistral_preflight_is_available`, and
`mistral_login::windows::browser_test::named_windows_keep_http_only_sessions_in_separate_stores`.
Use the existing WSL test distribution/installed-read environment flags; the
native browser fixture additionally requires `GAH_WINDOWS_BROWSER_TESTS=1`.

This implements the Windows path but does not establish actual Mistral login,
live allowance readings, automatic refresh over time, provider billing or other
providers' OAuth workflows. A real owner-driven sign-in remains acceptance work.
The live central and its asset delivery were not changed.
