#!/usr/bin/env bash
# Carry the Linux Node executable, compiled server and its locked production packages together.
set -euo pipefail
payload=$(realpath "${1:?pass the payload directory}")
optional=${2:?pass --include=optional or --omit=optional}
[[ $optional == --include=optional || $optional == --omit=optional ]] || exit 2
cd "$(dirname "$0")/.."
[[ $(uname -sm) == 'Linux x86_64' ]] || { echo 'Expected Linux x86_64.' >&2; exit 1; }
dist="$payload/apps/server/dist"
mkdir -p "$payload/bin" "$dist"
node_bin=$(node -p 'process.execPath')
install -m755 "$node_bin" "$payload/bin/node"
license="$(dirname "$node_bin")/../LICENSE"
if [[ ! -f $license && $node_bin == /usr/bin/node ]]; then license=/usr/share/doc/nodejs/copyright; fi
install -m644 "$license" "$payload/bin/NODE-LICENSE"
cp package.json "$payload/"
# A central source update can already have a worker kit in dist. Never put
# that kit inside the next kit. Runtime dependencies are built below.
tar -C apps/server/dist --exclude=./worker-artifacts --exclude=./node_modules --exclude=./.release-runtime -cf - . | tar -C "$dist" -xf -
test -s "$dist/bin.js"
node scripts/stage-release-runtime.mjs "$PWD" "$dist/.release-runtime"
(cd "$dist/.release-runtime" && npm ci --omit=dev "$optional" --workspace=apps/server --include-workspace-root=false --ignore-scripts --no-audit --no-fund)
node "$dist/.release-runtime/link.mjs"
git rev-parse HEAD > "$payload/REVISION"
