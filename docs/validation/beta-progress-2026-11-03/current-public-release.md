# Current public release candidate — Windows/WSL, 2026-11-05

The native desktop remains [source 018f6603](windows-updater-config.md), with
the repaired Windows updater lookup. Its separate Windows installer and closed
local test installation are unchanged by this CLI/server package update.

Source: `fdc9567f5a2062c277e8bd365764c3aa565b2375`, version 0.3.0.
Package: `artifacts/parasaur-central-fdc9567f.zip` (24,241,305 bytes).
SHA-256: `7fcd07b910a341a36dbbe353a1c38719906a062e4d91bd9abc3d219d8b32f331`.
This supersedes retained candidates `5e261e0e`, `43d73448`, `819937bf`,
`77209892` and historical `16e9ecc6`. It adds [claim evidence](kanban-claim-evidence.md),
[failed-refresh recovery and stale-worker activity](kanban-refresh-recovery.md)
plus [worker factory observations](worker-factory-observations.md) and
[streaming ledger reads](streamed-ledger-memory.md) to the matched package.
It also includes [server update recovery](server-update-recovery.md), resuming
persisted running updates and stopping status polling on unmount. Six Windows
updater cases passed against both public and exact assembled private sources.
All 29 JavaScript chunks served byte-for-byte
after actual isolated release installation, including the lazy page assets.

The package contains matched optimized Linux CLI/MCP binaries, the public
server/web bundle, tracked source, manifest, build/install records, checksums
and verification scripts/logs. It includes prior account/model/factory repairs
plus [bounded capture](rust-output-memory.md), [streaming event reads](event-reader-memory.md)
and [bounded controller passes](controller-pass-turnover.md), plus
[worker ledger release](worker-ledger-retention.md) and
[native Windows diagnostics](windows-memory-diagnostics.md). Private source
and overlays are excluded; enterprise remains a separate private build.

The fresh locked/offline optimized build passed **1,010 public server cases**
(two explicit skips) and **16 mock cases**, with no failures. The optimized CLI
passed the 128 MiB journal memory-limit check and all four review-format cases,
including oversized approval rejection. Debug scheduler-budget and pressure
hooks are absent from the release binary. These are local checks, not a daemon
soak or live OOM allocation attribution.

The actual updater passed isolated installation with deliberately old checkout
manifests and Windows npm first on PATH. It selected Linux npm, installed locked
dependencies, verified binary bytes and preserved the checkout manifests. Cold
HTML/JS/CSS bodies and byte ranges passed, along with packaged model recovery,
exact-account selection, uncertain-request retry and old-worker rejection checks.
Root/build-tool/service commands were blocked by fixture executables. All file
checksums, ZIP CRC and archived-file byte comparisons passed.

Both binaries require **glibc 2.39**; current Ubuntu-24.04 WSL meets that floor.
Live central compatibility remains unverified. Source/log:
`/home/khing/.cache/parasaur-public-release.f1s0dg15`.

The installed WSL user CLI at `~/.cargo/bin/gah` already matches the freshly
built candidate, SHA-256
`4cac1f30362fd072632f9dde9527183fff52a044e4335e308517b55949ab03e2`.
Rust source and tests are unchanged from `5e261e0e`; no user CLI replacement
was necessary. Rollback from that installation remains
`~/.cache/parasaur-upgrade-backups/gah-before-5e261e0e`.
The actual optimized CLI passed the memory regression and four review-format
cases again. Both native Windows integration reads passed after WSL recovery:
account metadata and Mistral preflight. `USER_INSTALL.json` and
`windows-installed-cli.log` record these checks. No provider sign-in, billing
call or GUI interaction occurred.

All six separate worker-release binary hashes remain unchanged. No worker
service was started, stopped or upgraded; no new user MCP installation was made.
The installed desktop remains SHA-256
`e768b6f72461a817f7c30f2ae101d4eb77ebd8d932314706de79d095cd27ad67`;
saved settings remain `4a773444c2dead36341accc15750b5eac2235b018d69b6bde345d08b81ac9caa`.
The desktop remains closed. Earlier hashes below belong to the historical package.

`BUILDINFO.json`, `INSTALL_CHECK.json` and `WINDOWS_STATE.json` retain exact checks
and hashes. The desktop bundles local Settings and loads its dashboard from the
configured central service; rebuilding its unchanged native executable would
not deliver these web fixes. The actual release installer was tested only in
an isolated checkout. This is an unsigned, unpublished local candidate. The
WSL user CLI matches; the native desktop and central are unchanged. Signed
updates, fresh-host installation,
authenticated live pipeline acceptance, real provider usage/billing, live memory
attribution, enterprise hosting and physical iPhone delivery remain required.

## Historical candidate — 16e9ecc6, 2026-11-04

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
