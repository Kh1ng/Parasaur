#!/usr/bin/env bash
# Run against the payload from the built AppImage in a stock offline Linux
# container. The bundled Node process drives both the API and dashboard checks.
set -euo pipefail
assets=$(realpath "${1:?pass the AppImage or its standalone resource directory}")
if [[ -f $assets ]]; then
  extracted=$(mktemp -d)
  trap 'rm -rf -- "$extracted"' EXIT
  (cd "$extracted" && "$assets" --appimage-extract >/dev/null)
  mapfile -t archives < <(find "$extracted" -type f -path '*/standalone/runtime.tar.gz')
  [[ ${#archives[@]} == 1 ]] || { echo 'Expected one standalone runtime in the AppImage.' >&2; exit 1; }
  assets=$(dirname "${archives[0]}")
fi
docker run --rm --network none --mount "type=bind,source=$assets,target=/bundle,readonly" -i "${2:-ubuntu:22.04}" bash -se <<'SMOKE'
set -euo pipefail
for tool in git node npm cargo rustc cc; do
  if command -v "$tool"; then echo "Unexpected test-host tool: $tool" >&2; exit 1; fi
done
export HOME=/tmp/tester
mkdir -p "$HOME" /tmp/runtime /tmp/state/config
(cd /bundle && sha256sum --check --strict SHA256SUMS)
tar -xzf /bundle/runtime.tar.gz -C /tmp/runtime
bin=/tmp/runtime/bin
"$bin/gah" config set --node-role standalone
"$bin/gah" status --role --json | "$bin/node" -e 'process.exit(JSON.parse(require("fs").readFileSync(0)).role === "standalone" ? 0 : 1)'
"$bin/gah-mcp-server" --version
cd /tmp/state
"$bin/serve" > /tmp/server.log 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT
if ! "$bin/node" --input-type=module <<'JS'
import assert from 'node:assert/strict';
import {setTimeout as wait} from 'node:timers/promises';
const origin = 'http://127.0.0.1:3773';
for (let n=0; n<60; n++) {
  try {
    const health = await (await fetch(origin+'/health')).json();
    assert.equal(health.status, 'healthy');
    assert.equal(health.checks.rustBackend.ready, true);
    const page = await fetch(origin);
    assert.equal(page.status, 200);
    const html = await page.text();
    assert.match(html, /id="root"/);
    const asset = html.match(/src="([^"]+\.js)"/)[1];
    assert.equal((await fetch(new URL(asset,origin))).status, 200);
    console.log('Offline standalone dashboard, JS asset and Rust backend passed.');
    process.exit(0);
  } catch (e) { if(n===59) throw e; await wait(500); }
}
JS
then cat /tmp/server.log >&2; exit 1; fi
SMOKE
