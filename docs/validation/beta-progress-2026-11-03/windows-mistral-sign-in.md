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

## Installed Windows candidate

The optimized NSIS/MSI build at `fa630df1` completed and the current-user NSIS
upgrade returned zero. Its desktop-only artifacts are staged under ignored
`artifacts/parasaur-desktop-fa630df1`. Installer SHA-256 is
`97028b557c4dc483d7e0631d8a5cf0af143d9978c8cf57f5cbad8792b3d04a43`.
Installed executable SHA-256 is
`4fe26dc5dc3aeba642412b73fd28b6bd372c0fcb922cb2230f9d01c76660a4da`,
matching the optimized build with only Tauri's expected NSIS bundle marker.
Saved settings stayed identical through installation and launch. The previously
installed WSL CLI at `b738b9d2` provides the existing commands used by this build;
these different candidate manifests must not be combined into one bundle.

The actual installed window returned to local Settings with the connection
timeout notice. A later UI check was interrupted by the user's Escape stop;
no real Mistral sign-in was attempted. Subsequent work is headless Windows-only
at the owner's request; MacBook and further desktop interaction are deferred.

## Connection-change guard (2026-11-04)

The Windows verification path now uses a compare-and-save protocol.
The helper snapshots an opaque connection token before making the provider
request. The CLI checks that token under the existing source lock before
publishing. Rotation, deletion, or another successful verification invalidates
earlier checks. A stale result leaves the newer connection and quota history
alone and tells the user to retry. The network request holds no credential lock.
Default-cookie checks also detect file replacement or metadata changes; custom
cookie sources remain protected. This requires the updated CLI and desktop
together; preflight rejects an older CLI before opening the provider window.
Unix native sign-in has not been migrated to this protocol.

After recovery, all 24 focused `credentials::` library tests passed against
the Windows working tree under Ubuntu-24.04, including all four publication
tests, interrupted-marker recovery and unchanged quota history after rejection.
All 35 default Windows native tests passed (six opt-in fixtures skipped).
The expanded Windows-to-WSL staging fixture passed both with its synthetic CLI
and with the newly built actual CLI for credential commands. It verified named
and default saves and rejected concurrent rotations without replacing newer
connections. No provider request or desktop window was involved.

Commands used the root `cargo test --lib credentials:: --locked --offline`,
root `cargo build --bin gah --locked --offline`, and the Windows opt-in staging
filter listed above. Core outputs now use persistent
`CARGO_TARGET_DIR=/home/khing/.cache/parasaur-beta-target`;
`GAH_WINDOWS_WSL_TEST_GAH` pointed at its `debug/gah` for the actual-CLI phase.
Windows desktop `cargo check --bin gah-desktop --tests --locked`, focused Rust
formatting, and both Python scripts' syntax checks also passed.

Runtime verification encountered a full C: drive (about 14 MB free) and a
Windows linker PDB LIMIT failure. Package-scoped `cargo clean --package
gah-desktop --profile dev` reclaimed 3.4 GiB of disposable development outputs;
release artifacts were retained. Ubuntu-24.04 continued reporting filesystem
I/O errors, with kernel write errors for its disk. Restarting the distro required
owner approval because it had other running Linux clients. After approval,
Ubuntu was terminated;
its first boot failed. With the sole registered distro stopped, WSL VM shutdown
and a second boot recovered execution and writable temporary storage. Windows
reported about 245 GiB free afterward. The former temporary build checkout was
gone. The new boot allowed the blocked checks to complete without desktop
interaction. Real owner-driven provider acceptance remains required.

## Matched installed update (2026-11-04)

Source `c1a55cf01c8d8928c87e8974282d99b86408c13f` built the optimized Windows
NSIS/MSI and Linux CLI. The combined ignored bundle
`artifacts/parasaur-windows-beta-c1a55cf0.zip` passed the actual installer's
manifest validation for all five routes, with matching revisions and hashes.
ZIP SHA-256 is
`ef74e043c1fb3c6de255a4e6022664dd5b88a8aa35525cd6fb4cf1bce828ed18`.
NSIS SHA-256 is
`fccb0d311346add7afed42f8d6f275a515d2449bf7109c8c668f6727603617d0`.

The verified user CLI was atomically replaced in Ubuntu-24.04, with its previous
binary retained in `~/.cache/parasaur-upgrade-backups/gah-before-c1a55cf0`.
Installed CLI SHA-256 is
`df7129070185a9f7399403a39d9f2e3a3d4963bd879a93def8ab476f4b344ba9`.
The current-user silent desktop upgrade returned zero and preserved the saved
settings hash. Installed desktop SHA-256 is
`bd40bb336a7a04f19b467974a0e0ac20c12ffbf07cb9d314bb9fd8e063179f63`;
the only difference from the raw build is Tauri's `UNK` to `NSS` bundle marker.
The application was left closed. No GUI automation or sign-in was performed.

Both opt-in read-only tests against the actual installed WSL CLI passed:
account metadata and the new Mistral protocol preflight. The real-CLI two-account
Windows-to-WSL save/isolation fixture also passed. No worker service was installed
or started. This is an unsigned, unpublished local update, not signed-update,
live provider, live factory, or current-build rendering acceptance.
