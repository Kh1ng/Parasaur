#!/usr/bin/env bash
# Launch a built GAH AppImage with a fresh profile and fail unless its first
# window renders the bundled Settings page instead of a blank view (#1326).
# Runs under Xvfb when DISPLAY is unset. Needs xvfb, xdotool and imagemagick.
#
# Usage: scripts/smoke-linux-appimage.sh path/to/GAH.AppImage [screenshot.png]
# The screenshot and the app's output (screenshot path with .log) stay behind.
set -euo pipefail

[ -f "${1:-}" ] || { echo "usage: $0 GAH.AppImage [screenshot.png]" >&2; exit 2; }
appimage=$(realpath "$1")
shot=$(realpath -m "${2:-appimage-smoke.png}")
if [ -z "${DISPLAY:-}" ]; then
  exec xvfb-run -a -s "-screen 0 1600x1000x24" "$0" "$appimage" "$shot"
fi
mkdir -p "$(dirname "$shot")"
log="${shot%.png}.log"
profile=$(mktemp -d)

# No config, server, CLI login or checkout: a fresh install must still show
# actionable onboarding. Extract-and-run avoids a FUSE requirement; TMPDIR
# keeps the extracted copy inside the profile so cleanup removes it.
HOME="$profile" XDG_CONFIG_HOME="$profile/config" XDG_DATA_HOME="$profile/data" \
  XDG_CACHE_HOME="$profile/cache" XDG_STATE_HOME="$profile/state" TMPDIR="$profile" \
  APPIMAGE_EXTRACT_AND_RUN=1 "$appimage" >"$log" 2>&1 &
pid=$!
trap 'pkill -P "$pid" || true; wait "$pid" 2>/dev/null || true; rm -rf "$profile"' EXIT

# A blank WebKit view is one flat colour; the Settings page has text. Look
# below the native menu bar so the menu's own text cannot pass the check.
colors=0
for _ in $(seq 60); do
  sleep 1
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "FAIL: the AppImage exited during startup. Output:" >&2
    tail -n 40 "$log" >&2
    exit 1
  fi
  window=$(xdotool search --onlyvisible --name '^GAH$' 2>/dev/null | head -n 1) || true
  [ -n "$window" ] || continue
  import -window "$window" "$shot" 2>/dev/null || continue
  colors=$(convert "$shot" -gravity south -crop 100%x75%+0+0 +repage -format %k info:)
  if [ "$colors" -gt 16 ]; then
    echo "PASS: the first window rendered content ($colors colours): $shot"
    exit 0
  fi
done
echo "FAIL: the first window stayed blank ($colors colours after 60 s): $shot" >&2
tail -n 40 "$log" >&2
exit 1
