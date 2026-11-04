# Current public release candidate — Windows/WSL, 2026-11-04

Source: `b0ea279f331e306418bfd0d2d55f40f8caa6ad55`, version 0.3.0.
The local immutable recovery package is
`artifacts/parasaur-central-b0ea279f.zip` (23,994,208 bytes).
SHA-256: `9bc1e9ac95809fc7455c9364e87dcb347354ab6cecfc0bbf36750d5f687a18f4`.

It contains matched optimized Linux CLI/MCP binaries, the public server/web
bundle, tracked source, a local release manifest, build information, individual
checksums and the actual install/smoke scripts. It includes the later public
model/account catalog repairs, execution-node account discovery, factory
diagnostic failure/deadline states and previously integrated factory/prefix
changes. It supersedes the older `d86a59b4` package for those server/web fixes.
The source export and runtime were checked to exclude private submodule/overlay
contents. Enterprise remains a separate private build.

## Build and actual release installation

A fresh Linux filesystem Git export built with locked/offline optimized Rust
and the committed npm lockfile. The normal server/web build gate passed:
**1,000 server tests, one opt-in skip, zero failures**, and **12 mock tests**.
Both executables' ELF symbol requirements were inspected: maximum required
glibc is **2.39**. Compatibility with the live host is not established.

The actual optimized updater installed this candidate into a disposable Git
checkout with deliberately old package manifests and Windows npm inherited
first on PATH. Root/service/build-tool commands were blocked by fixture
executables. Installation selected Linux npm, installed the release's locked
runtime dependencies, verified binary bytes and preserved checkout manifests.
The installed cold server returned complete dashboard HTML/JS/CSS bodies and
correct byte ranges. Packaged account Retry, factory-unavailable copy and the
account endpoint were present. No system service changed.

The candidate's `INSTALL_CHECK.json` and `BUILDINFO.json` record the checks.
Every individual SHA-256 check and ZIP integrity check passed. Build source/log:
`/home/khing/.cache/parasaur-public-release.6o8lxrc2`.

## Installed user CLI

The actual WSL user CLI was atomically replaced after verifying its recorded
prior bytes. Its new SHA-256 is
`a5e6c3cc9fe21bae4856889e0364342de74ecadf767f87233c4ba249f925ab96`.
Rollback copy: `~/.cache/parasaur-upgrade-backups/gah-before-b0ea279f`.
`gah --version` returned `parasaur 0.3.0`.

Both existing opt-in Windows tests against the installed WSL CLI passed:
account metadata and Mistral protocol preflight. No provider sign-in or worker
service started. The MCP binary is included in the package; no new user-level
MCP installation was added.

`git diff c1a55cf0 b0ea279f -- apps/desktop` is empty, so the existing installed
Windows native desktop was retained. Its SHA-256 remains
`bd40bb336a7a04f19b467974a0e0ac20c12ffbf07cb9d314bb9fd8e063179f63`.
The saved Windows `desktop.json` hash remained
`4a773444c2dead36341accc15750b5eac2235b018d69b6bde345d08b81ac9caa`.
The application remains closed; no native GUI was operated.

This is an unsigned, unpublished local candidate and local CLI update. The
central server/web bundle is not deployed. Signed updates, fresh-host installs,
real provider billing/execution, authenticated live factory acceptance,
enterprise hosted/isolation checks and physical iPhone delivery remain pending.
