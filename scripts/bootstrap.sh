#!/usr/bin/env bash
# The paste-and-go install for Linux and macOS:
#
#   curl -fsSL https://raw.githubusercontent.com/Kh1ng/git-agent-harness/main/scripts/bootstrap.sh | bash
#
# While the repository is private, fetch it with a token and pass it along:
#
#   curl -fsSL -H "Authorization: Bearer $GITHUB_TOKEN" \
#     https://raw.githubusercontent.com/Kh1ng/git-agent-harness/main/scripts/bootstrap.sh \
#     | GITHUB_TOKEN="$GITHUB_TOKEN" bash
#
# It needs only git and curl. It asks before installing Rust (needed to build
# gah), clones this repository, builds gah, and hands the terminal to
# `gah setup`, which asks what this machine is for, checks and offers
# everything else, installs the service, and adds a first project.
#
# The commands Parasaur's own Settings page generates pass their choices as
# environment variables on the `bash` side of the pipe; they become
# `gah setup` flags below, so setup asks only what they left open:
#   PARASAUR_NODE_ROLE=central|standalone|worker   PARASAUR_CENTRAL_URL=<url>
#   PARASAUR_GATEWAY_MODE=remote|colocated PARASAUR_GATEWAY_URL=<url> PARASAUR_GATEWAY_MEMORYCORE_PATH=<dir>
#   PARASAUR_GATEWAY_PROVIDER=ollama|openai PARASAUR_GATEWAY_ENDPOINT=<url> and the
#   PARASAUR_GATEWAY_LLM_MODEL / PARASAUR_GATEWAY_EMBEDDING_MODEL / _DIMENSIONS choices
# Secrets (COORDINATOR_TOKEN, PARASAUR_GATEWAY_API_KEY, PARASAUR_GATEWAY_LLM_API_KEY,
# PARASAUR_GATEWAY_EMBEDDING_API_KEY) stay
# in the environment; setup reads them there and never takes them as flags.
#
# PARASAUR_INSTALL_DIR overrides the clone location (default: ~/git-agent-harness).
# PARASAUR_YES=1 accepts every default and offer without prompting.
# Settings were named GAH_* before the product became Parasaur. Accept the old
# names: a GAH_X with no PARASAUR_X becomes PARASAUR_X.
for legacy_setting in "${!GAH_@}"; do
  current_setting="PARASAUR_${legacy_setting#GAH_}"
  [ -n "${!current_setting+set}" ] || export "$current_setting=${!legacy_setting}"
done
unset legacy_setting current_setting
set -euo pipefail

os="$(uname -s)"
case "$os" in
  Linux|Darwin) ;;
  *)
    echo "ERROR: this installer supports Linux and macOS. On Windows, use the Parasaur Windows installer from the releases page." >&2
    exit 1
    ;;
esac

# curl owns stdin (this script arrives through it), so questions go to the terminal.
if [ "${PARASAUR_YES:-}" != 1 ]; then
  if ! { true </dev/tty; } 2>/dev/null; then
    echo "ERROR: no terminal to ask questions on. Run this in a terminal, or set PARASAUR_YES=1 to accept every default." >&2
    exit 1
  fi
fi

ask() {
  [ "${PARASAUR_YES:-}" = 1 ] && return 0
  local answer
  printf '%s [Y/n] ' "$1" >/dev/tty
  read -r answer </dev/tty
  case "$answer" in ""|y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

if ! command -v git >/dev/null 2>&1; then
  if [ "$os" = Darwin ]; then
    echo "git is missing. Install Apple's command line tools with: xcode-select --install" >&2
  else
    echo "git is missing. Install it with your package manager (for example: sudo apt-get install -y git)." >&2
  fi
  echo "Then run this installer again." >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1; then
  echo "Parasaur is built from source with Rust, which is not installed."
  if ! ask "Install Rust for your user with rustup (no administrator password)?"; then
    echo "Install Rust from https://rustup.rs, then run this installer again." >&2
    exit 1
  fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

# While the repository is private, GITHUB_TOKEN (read access) authenticates
# the clone and later pulls. It reaches git through environment config, never
# argv or the saved remote URL.
if [ -n "${GITHUB_TOKEN:-}" ]; then
  basic="$(printf 'x-access-token:%s' "$GITHUB_TOKEN" | base64 | tr -d '\n')"
  export GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=http.https://github.com/.extraheader
  export GIT_CONFIG_VALUE_0="AUTHORIZATION: basic $basic"
fi

install_dir="${PARASAUR_INSTALL_DIR:-$HOME/git-agent-harness}"
if [ -d "$install_dir/.git" ]; then
  echo "Updating the existing checkout at $install_dir"
  git -C "$install_dir" pull --ff-only
else
  echo "Cloning git-agent-harness into $install_dir"
  # A shallow clone: an install needs today's tree, not history (early
  # commits carry large, since-ignored build artifacts; 1.7MB vs ~350MB).
  git clone --depth 1 https://github.com/Kh1ng/git-agent-harness.git "$install_dir"
fi

cd "$install_dir"
echo "Building gah. The first build takes a few minutes."
cargo build --locked --release --bin gah

args=(setup --source "$install_dir")
case "${PARASAUR_NODE_ROLE:-}" in
  central|standalone|worker) args+=(--role "$PARASAUR_NODE_ROLE") ;;
  "") ;;
  *) echo "ERROR: PARASAUR_NODE_ROLE must be central, standalone, or worker." >&2; exit 1 ;;
esac
[ -z "${PARASAUR_CENTRAL_URL:-}" ] || args+=(--central-url "$PARASAUR_CENTRAL_URL")
case "${PARASAUR_GATEWAY_MODE:-}" in
  remote|colocated) args+=(--memory "$PARASAUR_GATEWAY_MODE") ;;
  "") ;;
  *) echo "ERROR: PARASAUR_GATEWAY_MODE must be remote or colocated." >&2; exit 1 ;;
esac
[ -z "${PARASAUR_GATEWAY_URL:-}" ] || args+=(--gateway-url "$PARASAUR_GATEWAY_URL")
[ -z "${PARASAUR_GATEWAY_MEMORYCORE_PATH:-}" ] || args+=(--memorycore "$PARASAUR_GATEWAY_MEMORYCORE_PATH")
case "${PARASAUR_FACTORY_ENABLED:-}" in
  true|false) args+=(--factory-enabled "$PARASAUR_FACTORY_ENABLED") ;;
  "") ;;
  *) echo "ERROR: PARASAUR_FACTORY_ENABLED must be true or false." >&2; exit 1 ;;
esac
[ "${PARASAUR_YES:-}" != 1 ] || args+=(--yes)

if [ "${PARASAUR_YES:-}" = 1 ]; then
  exec target/release/gah "${args[@]}"
fi
exec target/release/gah "${args[@]}" </dev/tty
