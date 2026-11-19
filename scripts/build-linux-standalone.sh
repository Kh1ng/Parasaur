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
cargo build --locked --release --bin gah --bin gah-mcp-server
npm run build:server
npm run --workspace=apps/web build
mkdir -p "$payload/apps/web"
bash scripts/stage-linux-server-runtime.sh "$payload" --omit=optional
install -m755 "${CARGO_TARGET_DIR:-target}/release/gah" "${CARGO_TARGET_DIR:-target}/release/gah-mcp-server" "$payload/bin"
cp -R apps/web/dist "$payload/apps/web/"
if [[ -f apps/server/dist/worker-artifacts/worker-artifact.json ]]; then
  cp -R apps/server/dist/worker-artifacts "$payload/apps/server/dist/"
fi
# The server resolves shipped tools and templates relative to its JS files.
# Use Git to exclude local settings, credentials and generated dependencies.
git archive HEAD scripts packaging | tar -x -C "$payload"
install -m755 packaging/standalone/serve "$payload/bin/serve"
tar -czf "$stage/runtime.tar.gz" -C "$payload" .
(cd "$stage" && sha256sum runtime.tar.gz > SHA256SUMS)
install -m644 packaging/standalone/install.sh "$out/install.sh"
mv "$stage/runtime.tar.gz" "$stage/SHA256SUMS" "$out/"
