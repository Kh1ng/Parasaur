#!/usr/bin/env bash
# Launch a built GAH AppImage with a fresh profile and fail unless its first
# window shows actionable onboarding from the bundled Settings page (#1326):
# - the accessibility tree has the Settings document, a setup status that the
#   setup_check IPC call filled in, and an enabled install action, so a blank
#   view, an error page or a static shell whose JS/IPC failed does not pass;
# - the screenshot below the menu bar is not one flat colour, so a page that
#   exists but does not paint does not pass either.
# Runs under Xvfb with its own D-Bus session when DISPLAY is unset.
# Needs xvfb, dbus, at-spi2-core, python3-pyatspi, xdotool and imagemagick.
#
# Usage: scripts/smoke-linux-appimage.sh path/to/GAH.AppImage [screenshot.png]
# The screenshot and the app's output (screenshot path with .log) stay behind.
# GAH_SMOKE_SEED=dir copies dir into the profile first (for negative checks).
set -euo pipefail

[ -f "${1:-}" ] || { echo "usage: $0 GAH.AppImage [screenshot.png]" >&2; exit 2; }
appimage=$(realpath "$1")
shot=$(realpath -m "${2:-appimage-smoke.png}")
if [ -z "${DISPLAY:-}" ]; then
  exec dbus-run-session -- xvfb-run -a -s "-screen 0 1600x1000x24" "$0" "$appimage" "$shot"
fi
mkdir -p "$(dirname "$shot")"
log="${shot%.png}.log"
profile=$(mktemp -d)
[ -z "${GAH_SMOKE_SEED:-}" ] || cp -R "$GAH_SMOKE_SEED"/. "$profile"/

# No config, server, CLI login or checkout: a fresh install must still show
# actionable onboarding. Extract-and-run avoids a FUSE requirement; TMPDIR
# keeps the extracted copy inside the profile so cleanup removes it.
HOME="$profile" XDG_CONFIG_HOME="$profile/config" XDG_DATA_HOME="$profile/data" \
  XDG_CACHE_HOME="$profile/cache" XDG_STATE_HOME="$profile/state" TMPDIR="$profile" \
  APPIMAGE_EXTRACT_AND_RUN=1 "$appimage" >"$log" 2>&1 &
pid=$!
trap 'pkill -P "$pid" || true; wait "$pid" 2>/dev/null || true; rm -rf "$profile"' EXIT

# Prints what it found; exits 0 only when onboarding is ready.
onboarding() {
  python3 -I - "$1" <<'PY'
import sys
import pyatspi

ACTIONS = {"Set up standalone", "Install Parasaur in Terminal", "Finish setup in Terminal"}


def text(node):
    try:
        t = node.queryText()
        return t.getText(0, t.characterCount).strip()
    except Exception:
        return ""


def walk(node, depth=0):
    yield node
    if depth < 40:
        for child in node:
            if child is not None:
                yield from walk(child, depth + 1)


pid = int(sys.argv[1])
app = next((a for a in pyatspi.Registry.getDesktop(0)
            if a is not None and a.get_process_id() == pid), None)
nodes = list(walk(app)) if app is not None else []
settings = any(n.name == "Parasaur — Settings" for n in nodes)
section = next((n for n in nodes if n.name == "Set up this computer" and n.childCount), None)
inside = list(walk(section)) if section is not None else []
status = next((text(n) for n in inside if n.getRoleName() == "status bar"), "")
checked = bool(status) and not status.startswith("Checking what this computer has")
action = any(n.getRoleName() == "push button" and n.name in ACTIONS
             and n.getState().contains(pyatspi.STATE_ENABLED) for n in inside) \
    or any(text(n).startswith("Run in a terminal") for n in inside)
print(f"settings document={settings}, setup checked={checked}, install action={action}")
sys.exit(0 if settings and checked and action else 1)
PY
}

result="no Parasaur window yet"
colors=0
for _ in $(seq 90); do
  sleep 1
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "FAIL: the AppImage exited during startup. Output:" >&2
    tail -n 40 "$log" >&2
    exit 1
  fi
  app=$(pgrep -P "$pid" -x gah-desktop | head -n 1) || true
  window=$(xdotool search --onlyvisible --name '^Parasaur$' 2>/dev/null | head -n 1) || true
  [ -n "$app" ] && [ -n "$window" ] || continue
  ok=0
  result=$(onboarding "$app" 2>/dev/null) && ok=1
  import -window "$window" "$shot" 2>/dev/null || continue
  # Look below the native menu bar so the menu's own text cannot pass.
  colors=$(convert "$shot" -gravity south -crop 100%x75%+0+0 +repage -format %k info:)
  if [ "$ok" = 1 ] && [ "$colors" -gt 16 ]; then
    echo "PASS: onboarding is shown ($result; $colors colours): $shot"
    exit 0
  fi
done
echo "FAIL: no actionable onboarding after 90 s ($result; $colors colours): $shot" >&2
tail -n 40 "$log" >&2
exit 1
