#!/usr/bin/env bash
# Build the runtime carried by the Linux desktop. The target computer needs
# neither a checkout nor a package manager. Keep the release's npm tree intact.
set -euo pipefail
cd "$(dirname "$0")/.."
[[ $(uname -sm) == 'Linux x86_64' ]] || { echo 'The standalone desktop runtime requires Linux x86_64.' >&2; exit 1; }
out="$PWD/apps/desktop/target/standalone"
mkdir -p "$out"
stage=$(mktemp -d "$out/.build-XXXXXXXX")
trap 'rm -rf -- "$stage"' EXIT
payload="$stage/runtime"
dist="$payload/apps/server/dist"
cargo build --locked --release --bin gah --bin gah-mcp-server
npm run build:server
npm run --workspace=apps/web build
mkdir -p "$payload/bin" "$dist" "$payload/apps/web"
install -m755 "${CARGO_TARGET_DIR:-target}/release/gah" "${CARGO_TARGET_DIR:-target}/release/gah-mcp-server" "$payload/bin"
node_bin=$(node -p 'process.execPath')
install -m755 "$node_bin" "$payload/bin/node"
cp "$(dirname "$node_bin")/../LICENSE" "$payload/bin/NODE-LICENSE"
cp package.json "$payload/"
cp -R apps/server/dist/. "$dist/"
cp -R apps/web/dist "$payload/apps/web/"
rm -rf -- "$dist/node_modules" "$dist/.release-runtime"
node scripts/stage-release-runtime.mjs "$PWD" "$dist/.release-runtime"
# Standalone uses the user's chosen coding agents. Optional native provider
# downloads include both glibc and musl executables and are not server dependencies.
(cd "$dist/.release-runtime" && npm ci --omit=dev --omit=optional --workspace=apps/server --include-workspace-root=false --ignore-scripts --no-audit --no-fund)
node "$dist/.release-runtime/link.mjs"
# The server resolves shipped tools and templates relative to its JS files.
# Use Git to exclude local settings, credentials and generated dependencies.
git archive HEAD scripts packaging | tar -x -C "$payload"
install -m755 packaging/standalone/serve "$payload/bin/serve"
git rev-parse HEAD > "$payload/REVISION"
tar -czf "$stage/runtime.tar.gz" -C "$payload" .
(cd "$stage" && sha256sum runtime.tar.gz > SHA256SUMS)
install -m644 packaging/standalone/install.sh "$out/install.sh"
mv "$stage/runtime.tar.gz" "$stage/SHA256SUMS" "$out/"
