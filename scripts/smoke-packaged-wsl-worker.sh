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
trap 'systemctl --user stop gah-worker.service || true' EXIT
bash "$stage/install-wsl.sh" "$stage"
curl -fsS --retry 20 --retry-delay 1 --retry-connrefused http://127.0.0.1:3774/health | jq -e '.status == "healthy" and .checks.rustBackend.ready == true'
pid=$(systemctl --user show gah-worker.service -p MainPID --value)
node=$(readlink -f "/proc/$pid/exe")
[[ $node == "$root"/release.*/bin/node ]]
test "$(cat "$(dirname "$node")/../REVISION")" = "$(jq -r .revision "$kit/worker-artifact.json")"
echo 'The installed user service is healthy and running the bundled Node and Rust CLI.'
