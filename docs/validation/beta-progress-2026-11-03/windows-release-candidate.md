# Windows release candidate and actual per-user upgrade

Built the optimized Tauri Windows x64 app, NSIS installer and MSI from source
`bbc37aa5288f00e476e69ebc273e6d531b4a3037`, including native dashboard recovery
and the prebuilt WSL prerequisite fix. Desktop typechecking, platform builds,
installer contracts and artifact-staging contracts passed. The package is
unsigned and unpublished; no signed automatic-update feed was produced.

The local delivery file is
`artifacts/parasaur-windows-beta-bbc37aa5.zip` (ignored build artifact).
It contains a combined `bundle/`, the preview executable, alternate MSI,
BUILDINFO and README. Desktop and worker manifests identify the same source
revision; the real packaged installer validator accepted both manifests and
all five routes. The ZIP file allowlist and selected payload checksums passed.
The final ZIP SHA-256 is
`29e543c30a5446d2c7839d847a3f35e6338e93ce027bcb88b169a72b8053fe30`.
The source archive contains tracked public files and the updated prerequisite
script, with no private enterprise submodule contents or test profile data.

| Component | SHA-256 |
| --- | --- |
| NSIS `GAH_0.3.0_x64-setup.exe` | `d83456a30edc0fb8ca961d780107a0b6c6628cedb4ecd819e74bc6ed5b64ec1e` |
| MSI `GAH_0.3.0_x64_en-US.msi` | `583f4fc2928c8d78cc34f5e6b76dca6ebf39dc7ca89a31bddfaa8121860d30bb` |
| Raw preview `gah-desktop.exe` | `157f757c09fac99e9ad1f3d50c34e92a60a2566014aaf678edb1fed80482a5b2` |
| Linux worker CLI | `5438e161794b5a7b1c42c488e27f3502e8a91fe8dc586262574b86d5c20445f5` |
| Worker source archive | `9d3877d9fe378009aaf29c4093ce71ab1e6830b6290153b7c3d77b58980f54a4` |

## Validation and installation

The optimized app rendered recovery in an isolated Windows profile against a
loopback HTML document whose app script returned 503. Accessibility exposed the
heading, origin, retry button and local settings link. Its preview and fixture
server were stopped. Native link activation still has the previously recorded
input-geometry limitation; this does not claim a successful native click.

Registry inspection then confirmed an existing current-user GAH installation
at `C:\Users\cspur\AppData\Local\ParasaurNativeTest`. The generated NSIS
installer uses `currentUser` and `RequestExecutionLevel user`. After preserving
the old executable, registry metadata and saved-settings hash outside the
delivery package, the verified installer performed a silent same-version
upgrade to that exact directory and returned zero. This was an actual desktop
installation, not a mocked installer invocation. No worker setup, OS service
installation, administrator elevation or build-tool installation was performed.

The first raw executable comparison differed by three bytes. Inspection of
`tauri-utils` 2.10.2's `platform::bundle_type` confirmed Tauri patches the
unique `__TAURI_BUNDLE_TYPE_VAR_UNK` marker to `..._NSS` for NSIS. Applying only
that expected marker change to the preview bytes produced the exact installed
SHA-256 `30e1fd25d2e7cb10ec8f573cc4eba4e67138fd42e00747964f1ec6347fe300a1`.
There were no other byte differences, and the installed binary contains the
new recovery UI. The pre-upgrade executable did not contain it.

The installed app was started directly. Its process path and Parasaur window
were verified, with a native Settings menu present. The saved desktop settings
SHA-256 remained identical before/after installation and launch. The user still
has a separate legacy GAH Worker 0.1.1 installation: the UI launcher initially
opened that older app, so that newly opened extra instance was stopped and the
updated executable was started by exact path. The legacy installation itself
was not removed. The installer closed the earlier desktop preview processes;
the updated installed app was left running.

## Worker and remaining acceptance

The matching Linux worker CLI was built from the same immutable source and
passed its version, node-role, status-role and installer command contracts.
Its source archive passed both prerequisite regressions with Linux Node under
WSL, using command stubs rather than real apt/service operations. See
[prerequisite evidence](prebuilt-worker-prerequisites.md).

The local worker binary requires x86_64 Linux and glibc 2.39 (Ubuntu 24.04 build);
it is not an older-Linux compatibility artifact. A full worker installation
still needs an initialized compatible WSL distribution, central authorization
and normal setup permissions. It was packaged, not installed.

This verifies the per-user Windows desktop upgrade, not fresh-OS installation,
signed update delivery, native recovery-link activation or live dashboard
availability. The saved central remains the previously selected live address;
its deployed server was not upgraded. MacBook/live-factory checks, real provider
acceptance and physical iPhone delivery remain pending. No gate or live job was
changed, and no private credential was requested in this pass.
