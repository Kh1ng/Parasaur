# Current public release candidate — Windows/WSL, 2026-11-04

Source: `16e9ecc6f5ca8bd5a0db16c64574d1b6bde8dc17`, version 0.3.0.
The local immutable recovery package is
`artifacts/parasaur-central-16e9ecc6.zip` (24,037,747 bytes).
SHA-256: `f0272bb4f81d281c76c8c32325ac3089115f827692b4abb04532edd69ecfe135`.

It contains matched optimized Linux CLI/MCP binaries, the public server/web
bundle, tracked source, a local release manifest, build information, individual
checksums and the actual install/smoke scripts. It includes the later public
model/account catalog repairs, execution-node account discovery, factory
diagnostic failure/deadline states and previously integrated factory/prefix
changes. It also includes [exact factory account routing](factory-account-routing.md),
with node/account/model controls, receipt confirmation and safe identical retries.
It supersedes the older `b0ea279f` and `d86a59b4` packages.
The source export and runtime were checked to exclude private submodule/overlay
contents. Enterprise remains a separate private build.

## Build and actual release installation

A fresh Linux filesystem Git export built with locked/offline optimized Rust
and the committed npm lockfile. The normal server/web build gate passed:
**1,001 server tests, one opt-in skip, zero failures**, and **13 mock tests**.
Both executables' ELF symbol requirements were inspected: maximum required
glibc is **2.39**. Compatibility with the live host is not established.

The actual optimized updater installed this candidate into a disposable Git
checkout with deliberately old package manifests and Windows npm inherited
first on PATH. Root/service/build-tool commands were blocked by fixture
executables. Installation selected Linux npm, installed the release's locked
runtime dependencies, verified binary bytes and preserved checkout manifests.
The installed cold server returned complete dashboard HTML/JS/CSS bodies and
correct byte ranges. Packaged account Retry, factory-unavailable copy and the
account endpoint were present. The optimized CLI's help advertises
`--backend-instance`; the installed WorkPage assets contain the node/account
selection and pending/uncertain retry controls, and fleet transport contains
the old-worker rejection. No system service changed.

The candidate's `INSTALL_CHECK.json` and `BUILDINFO.json` record the checks.
Every individual SHA-256 check and ZIP integrity check passed. Build source/log:
`/home/khing/.cache/parasaur-public-release.su9wpmhk`.

## Installed user CLI

The actual WSL user CLI was atomically replaced after verifying its recorded
prior bytes. Its new SHA-256 is
`d34076b3f67f78f3492e95b000adef2c01d5462dafbbaaab1de3b1597be6504e`.
Rollback copy: `~/.cache/parasaur-upgrade-backups/gah-before-16e9ecc6`
preserves the previously installed `b0ea279f` CLI.
`gah --version` returned `parasaur 0.3.0`.

Both existing opt-in Windows tests against the installed WSL CLI passed:
account metadata and Mistral protocol preflight (`--ignored --exact` with the
existing opt-in environment switch). This update started no provider sign-in or
worker service. After the authorized WSL restart, an existing worker was observed
using its separate `~/.local/share/gah/worker/release.SzEWNWwv/bin/gah` binary;
that release, its service and its work were not changed. The MCP binary is
included in the package; no new user-level
MCP installation was added.

`git diff c1a55cf0 16e9ecc6 -- apps/desktop` is empty, so the existing installed
Windows native desktop was retained. Its SHA-256 remains
`bd40bb336a7a04f19b467974a0e0ac20c12ffbf07cb9d314bb9fd8e063179f63`.
The saved Windows `desktop.json` hash remained
`4a773444c2dead36341accc15750b5eac2235b018d69b6bde345d08b81ac9caa`.
The application remains closed; no native GUI was operated.

This is an unsigned, unpublished local candidate and local CLI update. The
central server/web bundle is not deployed. Signed updates, fresh-host installs,
real provider billing/execution, authenticated live factory acceptance,
enterprise hosted/isolation checks and physical iPhone delivery remain pending.
