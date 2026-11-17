#!/usr/bin/env bash
# Persist the installer's role and worker credential before services can restart.
# Remaining arguments select the current CLI (including cargo run on a fresh host).
# Settings were named GAH_* before the product became Parasaur. Accept the old
# names: a GAH_X with no PARASAUR_X becomes PARASAUR_X.
for legacy_setting in "${!GAH_@}"; do
  current_setting="PARASAUR_${legacy_setting#GAH_}"
  [ -n "${!current_setting+set}" ] || export "$current_setting=${!legacy_setting}"
done
unset legacy_setting current_setting
set -euo pipefail
role="${1:?Missing node role}"
shift
if [ "$role" = worker ]; then
  if [ -n "${PARASAUR_GATEWAY_MODE:-}" ]; then
    echo 'ERROR: workers use the central memory relay. Configure PARASAUR_CENTRAL_URL and COORDINATOR_TOKEN; configure PARASAUR_GATEWAY_MODE on central only.' >&2
    exit 1
  fi
  if [ -n "${PARASAUR_IMPORT_REPO:-}" ]; then
    echo 'ERROR: import shared project memory on the central node.' >&2
    exit 1
  fi
  credentials="$HOME/.config/gah/gah-loop.env"
  if [ -n "${COORDINATOR_TOKEN:-}" ]; then
    # The token travels on stdin; the file is written with mode 0600.
    printf '%s' "$COORDINATOR_TOKEN" | "$@" installer env-set --file "$credentials" COORDINATOR_TOKEN
  elif "$@" installer env-has --file "$credentials" COORDINATOR_TOKEN 2>/dev/null; then
    chmod 600 "$credentials"
  else
    echo 'ERROR: worker installation requires COORDINATOR_TOKEN for central claims and memory.' >&2
    exit 1
  fi
fi
args=(config set --node-role "$role")
# Existing configs retain legacy factory behavior. Fresh standalone apps opt out.
# macOS has no standalone role: its desktop installs central with PARASAUR_STANDALONE=1.
factory="${PARASAUR_FACTORY_ENABLED:-}"
standalone="${PARASAUR_STANDALONE:-}"
[ "$role" != standalone ] || standalone=1
if [ -z "$factory" ] && [ "$standalone" = 1 ] && [ ! -f "${PARASAUR_CONFIG:-$HOME/.config/gah/config.toml}" ]; then
  factory=false
fi
if [ -n "$factory" ]; then args+=(--factory-enabled "$factory"); fi
if [ -n "${PARASAUR_CENTRAL_URL:-}" ]; then args+=(--registry-central-url "$PARASAUR_CENTRAL_URL"); fi
"$@" "${args[@]}"
