#!/usr/bin/env bash
# Build the release server bundle consumed by `gah update --from-release`
# (issue #1416). Runs nothing itself: build the workspace outputs first
# (`npm run build:server && npm run build:web`; the MCP server is a Rust
# binary shipped as its own release asset since #1436),
# then this script validates them and archives the exact tree the release
# install path extracts over a checkout:
#
#   apps/server/dist          the node control-plane server
#   apps/web/dist            the dashboard
#   apps/server/dist/node_modules  compiled internal workspace imports
#   packaging/opencode/agents  the agent configs copied to ~/.config on update
#   package.json/package-lock.json  dependency-drift detection on install
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:?usage: build-release-bundle.sh <output.tar.gz>}"

expected=(
  apps/server/dist/bin.js
  apps/web/dist/index.html
  packages/contracts/dist/index.js
  packages/shared/dist/index.js
  packages/contracts/package.json
  packages/shared/package.json
  packaging/opencode/agents/gah-reviewer.md
  packaging/opencode/agents/gah-implementer.md
  package.json
  package-lock.json
)
for path in "${expected[@]}"; do
  if [ ! -f "$root/$path" ]; then
    echo "missing build output: $path" >&2
    echo "run: npm run build:server && npm run build:web" >&2
    exit 1
  fi
done

mkdir -p "$(dirname "$out")"
staging="$(mktemp -d)"
trap 'rm -rf -- "$staging"' EXIT
paths=(apps/server/dist apps/web/dist packaging/opencode/agents package.json package-lock.json)
for path in "${paths[@]}"; do
  mkdir -p "$staging/$(dirname "$path")"
  cp -R "$root/$path" "$staging/$path"
done
# Keep workspace imports inside the server output that existing updaters already
# install. A clean checkout has no ignored packages/*/dist to resolve at runtime.
rm -rf -- "$staging/apps/server/dist/node_modules"
for package in contracts shared; do
  target="$staging/apps/server/dist/node_modules/@git-agent-harness/$package"
  mkdir -p "$target"
  cp -R "$root/packages/$package/dist" "$target/dist"
  cp "$root/packages/$package/package.json" "$target/package.json"
done
# These are public runtime/build outputs, not credential files. A restrictive
# build umask must not leave assets unreadable to the service account.
chmod -R u+rwX,go+rX,go-w "$staging"
tar -czf "$out" -C "$staging" "${paths[@]}"
echo "wrote $out"
