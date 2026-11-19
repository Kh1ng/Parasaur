# Local Linux release candidate

All components were built from immutable public source revision
`1c0c38af78a4ae6d0edecedc1098c25f2022bde4`: optimized CLI, optimized MCP
server, compiled contracts/shared/server and the production web UI. Both
binaries report version 0.3.0, and the UI embeds the same source revision.
The release bundle includes isolated dependency runtime schema 1.

The local artifact directory is
`C:\Users\cspur\AppData\Local\Temp\parasaur-linux-beta-1c0c38af`.
It contains the three artifacts, `SHA256SUMS`, `local-manifest.json`,
`BUILDINFO.json`, source revision and README. Nothing was published or deployed.

| Artifact | SHA-256 |
| --- | --- |
| `gah-linux-x86_64` | `5438e161794b5a7b1c42c488e27f3502e8a91fe8dc586262574b86d5c20445f5` |
| `gah-mcp-server-linux-x86_64` | `d3012b30a8bea18cd5af8a91e12f16d701a91b1d0fdd20a34a978c5bebbc162e` |
| `gah-server-bundle.tar.gz` | `416da0d5193e4cbb8e35ad3dfded598a0914f7b5c0a6e8341cebffce6ab6720f` |

## Actual release installation

The optimized CLI executed its real public update command against an owned
disposable Git checkout with an old manifest, different old lockfile, old CLI
and old server output:

```sh
gah-linux-x86_64 update --repo <fixture>/repo --from-release \
  --release-manifest <artifacts>/local-manifest.json --role central --yes
```

HOME, XDG config/state, CARGO_HOME and the application config path were isolated.
A fixture command path made systemd unavailable and rejected any sudo/build
invocation. No real service installation, restart, provider execution or
remote-node operation was performed.

The command installed 653 npm packages into the staged release runtime,
installed the CLI/MCP server and `parasaur` alias, replaced the server/web
outputs, copied the bundled OpenCode agent definitions into the fixture config
and completed successfully. Installed binary bytes matched the candidate
artifacts; the alias reported 0.3.0. SHA-256 checks confirmed the checkout's
manifest and lockfile remained unchanged. No checkout-level node_modules was
created.

The installed Express application served HTML, referenced JavaScript
(`index-BrxuZ8bI.js`, 494,644 bytes) and CSS (`index-3va8HAL1.css`, 68,790
bytes) with HTTP 200 and complete bodies. A separate check used the compiled
entry point's `resolveWebRoot(undefined)` before constructing the server and
passed the same HTTP assertions. This verifies the actual packaged resolver
and HTTP layer; it does not claim a full production `bin.js` lifecycle or
provider-authentication acceptance. A first direct-constructor probe without
the resolver returned 404, as that lower-level API expects its caller to set
the web root; it was corrected rather than changing production behavior.

A separate clean extraction, without checkout node_modules or compiled
workspace outputs, also installed its own dependencies and passed HTTP checks.
The earlier updater/runtime regressions remain recorded in
[release runtime validation](release-runtime.md).

## Platform and remaining acceptance

These binaries were built under Ubuntu 24.04 in WSL. ELF dependency inspection
shows a minimum required glibc symbol version of **2.39**. This local candidate
is not a portable Linux release and has no Windows/macOS desktop installer or
signed desktop feed. A compatible release runner or another verified target
build is required for older Linux hosts. The target host's compatibility has
not been established.

The owner explicitly deferred MacBook work. The private SSH password request
was declined; no credential request is pending. No Mac or live-host deployment,
memory measurement, factory gate change or real provider job was performed.
