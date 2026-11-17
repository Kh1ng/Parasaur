# Windows updater configuration — 2026-11-05

Public source `018f6603c38de9a9d22c1581a882247e9257f419` fixes a reproduced
Windows updater configuration lookup failure. The updater previously used HOME
or a relative `.config` path, while the desktop uses USERPROFILE. With HOME unset
and a public-key file next to the Windows settings, the old code incorrectly
reported that no updater key was configured.

The updater now reuses the desktop settings directory unless XDG_CONFIG_HOME
is set. XDG, explicit config-file and environment-only configuration retain
their existing behavior. No endpoint, key, remote permission or signature
verification behavior was changed.

## Native verification

The new regression uses isolated Windows child processes, an unrelated working
directory and synthetic public-key strings; it never changes the parent's
environment or reads/writes real updater configuration. The profile case fails
with the old function's `updater not configured` error. With the repair, profile,
XDG, explicit-file and environment-only cases all pass. This tests discovery,
not cryptographic key validity or signed installation.

Windows `cargo test --manifest-path apps/desktop/Cargo.toml --locked --offline
-- --test-threads=1` passed **36 ordinary cases**, with eight explicit ignores.
`rustfmt --edition 2021 --check apps/desktop/desktop_update.rs` passed.
The production Settings bundle and `tauri build --bundles nsis` passed. Existing
Windows unused-app/WorkerState warnings remain; they are unrelated to this diff.
Only the updater module changed production code. Public CLI/server/web runtime
and the private source pair are unchanged, so their earlier checks were retained.

## Installed local desktop and retained candidate

The closed local test installation at
`C:\Users\cspur\AppData\Local\ParasaurNativeTest\gah-desktop.exe` was atomically
replaced with the verified native executable, with no GUI launch. Its SHA-256 is
`e768b6f72461a817f7c30f2ae101d4eb77ebd8d932314706de79d095cd27ad67`.
The previous `bd40bb33` executable is retained at
`artifacts/desktop-upgrade-backups/gah-before-018f6603.exe`.
Saved desktop settings still hash to
`4a773444c2dead36341accc15750b5eac2235b018d69b6bde345d08b81ac9caa`.
No provider account or WSL worker was changed.

Candidate: `artifacts/parasaur-desktop-018f6603.zip`, **19,232,956 bytes**.
SHA-256: `3666e91db7c62f86f3259ba4fc5a6c2ae0e141abee66d94fe6587fe16ea5c633`.
It contains the native executable, NSIS installer, public source export,
build/test logs, build/install metadata and individual checksums. All checksums,
ZIP CRC and archived-file byte comparisons passed; private source is excluded.
The installer SHA-256 is
`8dfe5890b14491c346f684a0a479d3703bf3498e9f8b37d3fa1096be219e48c9`.

The NSIS installer was built, not executed. This is an unsigned, unpublished
local build; Authenticode reports NotSigned. No publisher key/feed was provisioned,
and no signature/download/install/restart workflow or fresh-host install was
exercised. These remain release acceptance requirements. The live central and
shared dashboard were not deployed or changed.
