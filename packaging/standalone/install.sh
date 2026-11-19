#!/usr/bin/env bash
# Activate a verified local AppImage payload under the desktop user's service
# manager. User state stays outside the versioned runtime across updates.
set -euo pipefail
umask 077
assets=$(cd "$(dirname "$0")" && pwd)
root="$HOME/.local/share/parasaur/standalone"
config="$HOME/.config/gah/config.toml"
units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
service=parasaur-standalone.service
factory=${1:-}
fail() { echo "$*" >&2; exit 1; }
[[ $(uname -sm) == 'Linux x86_64' ]] || fail 'This package supports x86-64 Linux.'
[[ -z $factory || $factory == true || $factory == false ]] || fail 'Invalid factory setting.'
systemctl --user show-environment >/dev/null || fail 'Sign in to a Linux session with a systemd user manager, then try again.'
mkdir -p "$root/releases" "$root/state/config" "$units"
exec 9>"$root/install.lock"
flock -n 9 || fail 'Another standalone installation is running.'
read -r digest archive < "$assets/SHA256SUMS"
[[ $digest =~ ^[a-f0-9]{64}$ && $archive == runtime.tar.gz ]] || fail 'The standalone checksum file is invalid.'
(cd "$assets" && printf '%s  runtime.tar.gz\n' "$digest" | sha256sum --check --status) || fail 'The standalone package is damaged. Download the AppImage again.'
tmp=$(mktemp -d "$root/.install-XXXXXXXX")
trap 'rm -rf -- "$tmp"' EXIT
release="$root/releases/$digest"
if [[ ! -d $release ]]; then
  mkdir "$tmp/runtime"
  tar -xzf "$assets/runtime.tar.gz" -C "$tmp/runtime" --no-same-owner
  "$tmp/runtime/bin/node" --version >/dev/null
  "$tmp/runtime/bin/gah" --version >/dev/null
  [[ -f $tmp/runtime/apps/web/dist/index.html && -f $tmp/runtime/apps/server/dist/bin.js ]] || fail 'The standalone package is incomplete.'
  mv "$tmp/runtime" "$release"
fi
previous=$(readlink "$root/current" || true)
if [[ -f $config ]]; then
  status=$("$release/bin/gah" status --role --json --config-path "$config")
  role=$("$release/bin/node" -pe 'JSON.parse(require("fs").readFileSync(0)).role' <<<"$status")
  [[ $role == standalone ]] || fail "This account is configured as a $role node. Connect to that node or use a separate account for standalone."
else
  factory=${factory:-false}
fi
active=false
if systemctl --user is-active --quiet "$service"; then active=true; fi
if [[ $active == false ]]; then
  "$release/bin/node" -e 'const s=require("net").createServer();s.once("error",()=>process.exit(1));s.listen(3773,"127.0.0.1",()=>s.close())' ||
    fail 'Port 3773 is already in use. Connect to the existing dashboard or stop that service first.'
elif [[ $previous != "$release" && -n $(systemctl --user list-units --state=active --no-legend 'gah-loop@*.service') ]]; then
  fail 'Stop active project loops before updating the standalone server.'
fi
cat > "$tmp/unit" <<'UNIT'
[Unit]
Description=Parasaur standalone dashboard
[Service]
WorkingDirectory=%h/.local/share/parasaur/standalone/state
ExecStart="%h/.local/share/parasaur/standalone/current/bin/serve"
Restart=on-failure
RestartSec=3
[Install]
WantedBy=default.target
UNIT
if [[ ! -e $units/$service ]]; then
  install -m644 "$tmp/unit" "$units/$service"
elif ! cmp -s "$tmp/unit" "$units/$service"; then
  fail "An existing $service has different settings. Inspect it with systemctl --user cat $service."
fi
args=(config set --config "$config" --node-role standalone)
[[ -z $factory ]] || args+=(--factory-enabled "$factory")
"$release/bin/gah" "${args[@]}"
select_release() { ln -s "$1" "$tmp/current"; mv -Tf "$tmp/current" "$root/current"; }
healthy() {
  systemctl --user is-active --quiet "$service" &&
    "$release/bin/node" --input-type=module -e 'try {
      const r=await fetch("http://127.0.0.1:3773/health",{signal:AbortSignal.timeout(2000)});
      const h=await r.json(); process.exit(r.ok && h.status==="healthy" && h.checks.rustBackend.ready ? 0 : 1);
    } catch { process.exit(1); }'
}
if [[ $previous == "$release" && $active == true ]] && healthy; then exit 0; fi
select_release "$release"
if systemctl --user daemon-reload && systemctl --user enable "$service" && systemctl --user restart "$service"; then
  for ((i=0; i<60; i++)); do
    if healthy; then
      for candidate in "$root"/releases/*; do
        if [[ ${candidate##*/} =~ ^[0-9a-f]{64}$ && -f $candidate/REVISION && $candidate != "$release" && $candidate != "$previous" ]]; then
          rm -rf -- "$candidate"
        fi
      done
      exit 0
    fi
    sleep 0.5
  done
fi
# Roll back only this install's user service. Never restart a system-wide node.
if [[ -n $previous && $previous != "$release" ]]; then
  select_release "$previous"
  if [[ $active == true ]]; then systemctl --user restart "$service"; else systemctl --user stop "$service"; fi
elif [[ -z $previous ]]; then
  systemctl --user disable --now "$service" || true
  rm -f "$root/current"
fi
fail "Standalone did not become healthy. Inspect journalctl --user -u $service and retry setup."
