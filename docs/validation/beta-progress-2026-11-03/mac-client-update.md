# MacBook client update and temporary central test - 2026-11-05

The arm64 MacBook now has the current native desktop and CLI 0.3.0 installed.
The native app is running at `~/Applications/GAH.app`; its installed executable
and an on-screen normal window were observed. Window contents were not visually
inspected. This verifies launch, not a working authenticated live dashboard.
The live central still reports 0.2.0 and fails to serve its referenced assets.

Runtime source is `4c7d86ac`, exported from the Windows finishing branch without
altering the Mac's checkout. Installer repair `ccab1520` changes the shell
installer and its regression test only; Rust/native/shared-web runtime source
is unchanged. The build used macOS 27.2, arm64, Cargo 1.96 and Node 26.12.1.
This is a local ad-hoc-signed build, without Developer ID signing/notarization.

## Installation and storage

The prior CLI and desktop were 0.2.0. Backups of the old app, CLI and settings
are retained under `~/.local/state/gah/mac-client-backup-4c7d86ac-20261105/`.
The new installed CLI SHA-256 is
`5b404ea6c8124f9371266ce57dbd8b4f83ab8598853ae78214ada3a1d44b52ff`.
The sealed installed desktop executable SHA-256 is
`3f0ca60171413e5fe837c6f1122316b1bb551b9a3cccff8996371f94ce851251`.
Both the installed app signature and CLI version passed verification.

The original TOML config is byte-for-byte unchanged and retains worker role.
No execution worker or central service was installed or started. The desktop's
stale plain-IP URL was aligned with its existing configured central origin,
`https://hermesagent.tail82695.ts.net:8443/`. Presence and repository preferences
were retained. Credential files were not copied into build/test artifacts or
modified by the installer. The existing checkout, runtime source and unrelated
caches were not deleted.

A unique owned cache directory contained all source exports, npm dependencies,
Rust targets and central fixtures. Builds used three jobs, no incremental Rust
cache and a guard that stopped owned build processes below 8 GiB free space.
The guard did not fire. After transferring and hash-verifying evidence on
Windows, that entire temporary directory was removed. Cleanup verified the
central's ephemeral port closed, the client remained running, its signature and
binary hashes still matched, and original TOML bytes were unchanged.
Free space after cleanup was 19,054,874,624 bytes (about 17.7 GiB).
The installed client/CLI and bounded rollback material are retained; shared
package caches were preserved.

## Reproduced installer repair

The freshly bundled local app initially failed strict signature verification:
the Mach-O linker signature did not seal the app resources. The shared desktop
installer now verifies the staged app before replacing the installed one. It
seals a local linker-only ad-hoc signature, preserves already valid signatures,
and rejects damaged distribution signatures without replacing the installed app.
The source bundle stays unchanged; staging/backup cleanup remains atomic.

A new regression failed against the old installer, then both focused installer
cases passed on the Mac, including all three signature scenarios. The actual
Mac installation exercised the linker-only sealing branch and the installed
bundle passed `codesign --verify --deep --strict`. A raw Windows-to-Mac shell
file transfer initially carried CRLF; the transferred shell file was normalized
to LF before the passing checks and installation. A verification helper also
initially expected the old `gah` branding; the actual new command correctly
reports `parasaur 0.3.0` and its bytes/version were checked before recording success.

## Temporary central acceptance

The native Mac build passed 46 desktop tests, desktop typechecking, the app
bundle build and two signed-build script fixture cases. The server/web build
passed 1,011 server tests (two explicit skips), 16 mock tests, package builds
and production web compilation.

A real central `bin.js` process ran only on a temporary loopback port with an
isolated HOME/config/ledger/state, factory disabled and no real provider keys.
The test verified 0.3.0 central identity/health, invalid-token rejection, valid
owner-token access, an authenticated WebSocket, HTML/CSS/JS and range reads,
and all 29 JavaScript chunks byte-for-byte. A separate real-CLI/HTTP quota
fixture passed exact-account costs, unattributed history, estimates, unknowns,
measured zero and profile/period filtering. Neither test made provider calls.
The central exited successfully, its port was closed, and its files were removed.
No launchd central service or tailnet endpoint was created for these tests.

The configured live central is separate: health is 200/version 0.2.0, while its
referenced main JS/CSS return 500 through the HTTPS tailnet address and 403 at
the saved plain-IP origin. The local tests do not repair or prove acceptance of
that live deployment. Native client rendering, authenticated live factory work,
real providers, signed release distribution and a long client-memory soak remain
unverified. Mac memory issue #1680 is not closed by one running-process snapshot.

Evidence and a retained local client bundle are on Windows under
`artifacts/macos-client-4c7d86ac/`, including build/test logs, source/build/install
metadata, the failing/passing installer regression, runtime results and cleanup
record. The client archive SHA-256 is
`42b06d5fc534341da5fc5f843ee59dfdd6d92fe37f74479f4b92de193a18dfac`.
The evidence archive SHA-256 is
`1f03eb5e1e7722ee3d469f2de6f58ebafd751f1b9b10c2f4228f4b204f5b96d6`.
