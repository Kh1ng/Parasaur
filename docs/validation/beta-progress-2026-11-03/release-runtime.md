# Release runtime validation

The public server bundle previously omitted the compiled `contracts` and
`shared` workspace packages. A cold checkout with matching npm dependencies
but no workspace build outputs failed to import the actual server with
`ERR_MODULE_NOT_FOUND`. The corrected bundle embeds these packages under
`apps/server/dist/node_modules`, within the directory existing release
installers already replace. No installer protocol change is required.

Staging discards stale embedded dependencies and makes public build outputs
service-readable without changing the original build tree. The server's npm
`prebuild` hook removes the embedded package namespace when returning to a
source build, preventing old release imports from shadowing new workspace
outputs. Cleanup rejects resolved paths outside the server directory.

## Checks

- Seven Linux packaging/cleanup regressions passed, including cold package
  resolution, missing output, restrictive permissions, stale embedded imports,
  source cleanup and an escaping symlink. Both cleanup tests also passed on
  native Windows. Shell syntax validation passed.
- A clean public source archive at `78d5135f` built with Node 22.22.3 and
  matching npm dependencies. Its original release archive reproduced the
  missing-runtime failure. Repacking those outputs with the corrected script
  let the actual server load without workspace build outputs.
- The real Express server served HTML, referenced JavaScript (494,644 bytes)
  and CSS (68,790 bytes) with HTTP 200 and complete bodies. The web build
  identifies source revision `78d5135f`.
- That cold checkout then passed `npm run build:server`, invoking the new
  cleanup hook. The embedded namespace was absent afterward, and a fresh
  server process passed the same HTTP smoke check using source-built packages.
- All sixteen existing Rust release updater tests passed; no Rust installer
  changes remain in this patch.

The locally saved `gah-server-bundle.tar.gz` contains public server/web outputs
from `78d5135f`, including the memory retention fix. Its accompanying
`BUILDINFO.json` records its checksum and packaging revision. It is not
published or deployed and contains no CLI binary or private enterprise source.

## Remaining deployment constraints

The bundle still requires a compatible external npm dependency tree. These
checks used matching dependencies; they do not establish upgrades across
dependency-lock changes. A subsequent updater fix now rejects mismatched or
unreadable lockfiles before replacing any artifact. Previously it installed
the new CLI and server and then ran `npm ci` against the checkout's old
lockfile, leaving the new code paired with old dependencies. Nineteen release
tests and Rust formatting passed, including actual local-manifest installs
that verify dependency drift or a missing lockfile preserves the old CLI and
server and creates no MCP server, alias or new dashboard. This is a safety
fix; automatic dependency refresh across lock changes remains unfinished.
The complete updater unit group also passed (85 tests), and all-target,
all-feature Clippy passed with warnings denied. These checks ran in the
independent WSL test checkout against the current changed source files.

The saved bundle's `BUILDINFO.json` still identifies the earlier server source
and packaging revisions. It contains no CLI binary and therefore does not
carry the subsequent updater preflight fix.

The live v0.2.0 host's JavaScript/CSS GET failures remain unresolved. Permission
normalization is useful packaging hardening, not proof of their cause. Host
logs, authenticated deployment, and real Mac memory measurements remain
required. No service, live issue gate, or Tailscale listener was changed.
