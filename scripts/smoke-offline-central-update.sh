#!/usr/bin/env bash
# Install the Linux control plane using only the files about to be published.
set -euo pipefail
feed=$(realpath "${1:?pass the release asset directory}")
bootstrap=${2:-"$feed/gah-linux-x86_64"}
work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
controller="$work/controller"
export HOME="$work/home" CARGO_HOME="$work/cargo" XDG_CONFIG_HOME="$work/config" XDG_STATE_HOME="$work/state"
export PARASAUR_BINARY="$controller/bin/gah" PARASAUR_WEB_DEPLOY_ROOT=''
mkdir -p "$controller"/{apps/server/dist,apps/web/dist,bin} "$work/path" "$HOME"
printf '{"name":"git-agent-harness"}\n' > "$controller/package.json"
touch "$controller/apps/server/dist/bin.js" "$controller/apps/web/dist/index.html"
install -m755 "$bootstrap" "$PARASAUR_BINARY"
printf '#!/bin/sh\necho "Unexpected installer command: ${0##*/}" >&2\nexit 97\n' > "$work/path/npm"
chmod +x "$work/path/npm"
ln -s npm "$work/path/sudo"
export PATH="$work/path:$PATH"
jq --arg dir "$feed" '.assets |= map(.url = ($dir + "/" + .name))' "$feed/edge-manifest.json" > "$work/manifest.json"
"$PARASAUR_BINARY" update --role central --repo "$controller" --from-release --release-manifest "$work/manifest.json" --yes
cmp "$PARASAUR_BINARY" "$feed/gah-linux-x86_64"
cmp "$CARGO_HOME/bin/gah-mcp-server" "$feed/gah-mcp-server-linux-x86_64"
test -s "$controller/apps/web/dist/index.html"
(cd "$controller/apps/server/dist" && node --input-type=module -e "await import('express'); await import('@git-agent-harness/contracts')")
