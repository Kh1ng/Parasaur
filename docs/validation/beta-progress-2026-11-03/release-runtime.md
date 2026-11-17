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

## Initial packaging checks

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
- All sixteen existing Rust release updater tests passed. That initial
  packaging correction did not change the Rust installer; subsequent updater
  changes are described below.

The locally saved `gah-server-bundle.tar.gz` contains public server/web outputs
from `78d5135f`, including the memory retention fix. Its accompanying
`BUILDINFO.json` records its checksum and packaging revision. It is not
published or deployed and contains no CLI binary or private enterprise source.

## Remaining deployment constraints

The earlier bundle still requires a compatible external npm dependency tree.
Its checks used matching dependencies; they do not establish upgrades across
dependency-lock changes. A subsequent updater fix rejects mismatched or
unreadable lockfiles before replacing any artifact. Previously it installed
the new CLI and server and then ran `npm ci` against the checkout's old
lockfile, leaving the new code paired with old dependencies. Nineteen release
tests and Rust formatting passed, including actual local-manifest installs
that verify dependency drift or a missing lockfile preserves the old CLI and
server and creates no MCP server, alias or new dashboard. This is a safety
fix for legacy bundles; the subsequent isolated runtime below supplies dependency
installation for new bundles.
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

## Isolated release dependencies

New bundles additionally carry `apps/server/dist/.release-runtime` with the
release's exact root manifest/lockfile, every locked workspace manifest and
the compiled internal packages. Old installed dependencies are excluded when
packing. Workspace paths are checked before copying. No private npm config,
credentials or enterprise source is included.

The updater runs npm in that staged tree, then creates relative module links
for the server's root and workspace-local dependencies. Internal packages keep
their own complete npm lookup paths, including different nested versions and
optional platform dependencies. The server's compiled location, checkout
working directory and relative paths remain unchanged. The complete tree moves
with `dist`, so checkout manifests, lockfile and node_modules remain untouched.
Dependency installation finishes before any binary or dist swap. Legacy
bundles continue to require matching readable lockfiles.

A cold fixture installed all 653 npm packages from the release metadata with
no checkout-level node_modules or compiled workspace outputs. Its actual
Express server served HTML and the referenced JS/CSS with HTTP 200. Moving
that prepared dist into the earlier source fixture preserved those results.
A subsequent source build removed the isolated tree and module links, rebuilt
successfully, and passed the same real HTTP check. The server/web outputs in
this experiment came from `78d5135f`; this is not a claim that a new CLI was
deployed.

Regression coverage includes a real npm workspace install with a newer local
package than the checkout, unchanged checkout manifests/dependencies, relative
links after moving output, distinct hoisted/workspace/transitive versions,
escaping workspace paths, unknown runtime schema, inconsistent runtime locks,
stale dependencies excluded from packing, and npm failure leaving the old CLI,
server and other artifacts intact. Source cleanup also passed on Windows.
The release workflow now runs the packaging, linking and cleanup tests.

Final verification passed all 88 updater unit tests, eleven Linux runtime/
packaging/cleanup tests, four Windows cleanup tests, Rust formatting,
all-target/all-feature Clippy with warnings denied, shell syntax, workflow YAML
parsing and diff whitespace checks. The full server suite was not repeated for
these packaging/updater-only changes; its prior memory-fix results remain
recorded separately.

An updated CLI is required to use this installation format. Older updater
processes retain their old behavior even if the downloaded CLI is newer;
use the source update path or install the new CLI first for that transition.
Registry access or a sufficient npm cache is needed. Live installation,
service restart and Mac acceptance remain outstanding.
