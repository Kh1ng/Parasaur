#!/usr/bin/env bash
# Exercise the WSL installer with a real systemd user service on a fresh hosted Ubuntu runner.
set -euo pipefail
kit=$(realpath "${1:?pass the worker kit directory}")
[[ ${GITHUB_ACTIONS:-} == true && ${RUNNER_ENVIRONMENT:-} == github-hosted ]] || { echo 'Requires a disposable GitHub-hosted runner.' >&2; exit 1; }
root="$HOME/.local/share/gah/worker"
[[ ! -e $root && ! -e $HOME/.config/systemd/user/gah-worker.service ]] || { echo 'Worker already installed.' >&2; exit 1; }
stage=$(mktemp -d "$RUNNER_TEMP/wsl-worker-XXXXXXXX")
for file in gah source.tar.gz install-wsl.sh; do ln -s "$kit/$file" "$stage/$file"; done
printf '%s\n' '{"central_url":"http://127.0.0.1:1","token":"local-test","display_name":"packaged worker","advertised_url":"http://127.0.0.1:3774"}' > "$stage/settings.json"
mkdir "$stage/path"
printf '%s\n' '#!/bin/sh' 'echo "unexpected npm on the worker" >&2' 'exit 1' > "$stage/path/npm"
chmod +x "$stage/path/npm"
export PATH="$stage/path:/usr/bin:/bin" XDG_RUNTIME_DIR="/run/user/$(id -u)"
sudo loginctl enable-linger "$(id -un)"
sudo systemctl start "user@$(id -u).service"
# Exercise the release feed with the same binaries the workflow will publish.
# The machine's service gets the manifest from its environment like a pinned fleet.
package="$stage/feed"
mkdir -p "$package/apps/server/dist/worker-artifacts"
for file in gah source.tar.gz; do ln "$kit/$file" "$package/apps/server/dist/worker-artifacts/$file" || cp "$kit/$file" "$package/apps/server/dist/worker-artifacts/$file"; done
tar -czf "$package/gah-server-bundle.tar.gz" -C "$package" apps/server/dist/worker-artifacts
revision=$(jq -er .revision "$kit/worker-artifact.json")
bundle_sha=$(sha256sum "$package/gah-server-bundle.tar.gz" | cut -d' ' -f1)
jq -n --arg commit "$revision" --arg bundle "$bundle_sha" --arg cli "$(jq -er .files.gah "$kit/worker-artifact.json")" \
  --arg version "$(jq -er .version package.json)" \
  '{schema:1,channel:"test",version:$version,commit:$commit,assets:[
    {name:"gah-server-bundle.tar.gz",kind:"server-bundle",sha256:$bundle},
    {name:"gah-linux-x86_64",kind:"cli",sha256:$cli}]}' > "$package/edge-manifest.json"
systemctl --user set-environment PARASAUR_RELEASE_MANIFEST="$package/edge-manifest.json"
trap 'systemctl --user stop gah-worker.service || true' EXIT
bash "$stage/install-wsl.sh" "$stage"
curl -fsS --retry 20 --retry-delay 1 --retry-connrefused http://127.0.0.1:3774/health | jq -e '.status == "healthy" and .checks.rustBackend.ready == true'
pid=$(systemctl --user show gah-worker.service -p MainPID --value)
node=$(readlink -f "/proc/$pid/exe")
[[ $node == "$root"/release.*/bin/node ]]
test "$(cat "$(dirname "$node")/../REVISION")" = "$(jq -r .revision "$kit/worker-artifact.json")"
echo 'The installed user service is healthy and running the bundled Node and Rust CLI.'

identity=$(sha256sum "$root/identity.json")
update=http://127.0.0.1:3774/api/worker-update
curl -fsS -H 'Content-Type: application/json' -d '{"action":"start"}' "$update" | jq -e '.started'
update_status() {
  curl -fsS -H 'Content-Type: application/json' -d '{"action":"status"}' "$update"
}
for _ in {1..120}; do
  new_pid=$(systemctl --user show gah-worker.service -p MainPID --value)
  [[ $new_pid != "$pid" && $(readlink -f "/proc/$new_pid/exe" || true) == "$root/release.$revision.$bundle_sha/bin/node" ]] && break
  if status=$(update_status 2>/dev/null) && jq -e '.status.status == "failed"' <<< "$status" >/dev/null; then
    jq '.status | {status, exit_code, output: .output[-1600:]}' <<< "$status" >&2
    exit 1
  fi
  sleep 1
done
if [[ $(readlink -f "/proc/$new_pid/exe" || true) != "$root/release.$revision.$bundle_sha/bin/node" || $new_pid == "$pid" ]]; then
  update_status | jq '.status | {status, active_dispatches, exit_code, output: .output[-1600:]}' >&2
  exit 1
fi
curl -fsS --retry 20 --retry-delay 1 --retry-connrefused http://127.0.0.1:3774/health | jq -e '.status == "healthy" and .checks.rustBackend.ready == true'
update_status | jq -e '.status.status == "success"'
[[ $(sha256sum "$root/identity.json") == "$identity" ]]
echo 'The dashboard updated the enrolled worker and systemd started the verified release with the same identity.'
