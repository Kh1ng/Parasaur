# Agent CLI versions and updates

Each node checks its installed CLIs. The Agents page shows installed and latest versions, missing CLIs, update errors, and unavailable nodes.

For a local report, run:

```sh
gah agents versions --json
```

Each CLI or registry probe has a ten-second deadline. Probes run concurrently in a separate process. The server caches results for five minutes.

## Version sources

| CLI | Latest version source | Update command |
| --- | --- | --- |
| Claude | `npm view @anthropic-ai/claude-code version --json` | `claude update` |
| Codex | `npm view @openai/codex version --json` | `codex update`, or npm/Homebrew for those installations |
| OpenCode | `npm view opencode-ai version --json`; `@opencode/cli` for installed v2+ | `opencode upgrade` |
| agy | Platform manifest from Google's CLI updater | `agy update` |
| Vibe | `info.version` from PyPI's `mistral-vibe` JSON endpoint | `uv tool upgrade mistral-vibe`, or `brew upgrade mistral-vibe` for Homebrew installations |

The agy manifest URL comes from the [official installer](https://antigravity.google/cli/install.sh). Its `update` command has no documented read-only check flag.

OpenCode uses its [documented upgrade command](https://dev.opencode.ai/docs/cli) and the [v2 package](https://dev.opencode.ai/v2/docs/) for installed v2 CLIs. Vibe uses its [documented package manager update](https://docs.mistral.ai/vibe/code/cli/install-setup).

The report includes OpenHands and Hermes installed versions. It explicitly marks their latest-version checks and updates as unsupported.

## Owner updates

For an update plan, run:

```sh
gah agents update codex
```

The command prints the proposed update and exits without changing the CLI. To accept it, add `--yes`:

```sh
gah agents update codex --yes
```

The Agents page offers an Update button for outdated CLIs. This action requires owner access and `GAH_ENABLE_ADMIN_UPDATE=1` on central and the target node.

Updates use the node service account and its PATH. They do not use sudo or install missing CLIs. Provider errors and timeouts remain visible.

Dashboard and scheduled updates defer while dispatch claims are active or unreadable. A shared process lock prevents concurrent CLI and GAH installations.

An installer has a ten-minute deadline. GAH checks the installed version after the installer exits successfully.

Update state is private and atomic under `$XDG_STATE_HOME/gah/agent-cli-updates/`. Without `XDG_STATE_HOME`, GAH uses `~/.local/state/gah/agent-cli-updates/`.

Each backend has a JSON record with its command, versions, timestamps, and outcome. This state survives server restarts. A vanished updater is a failure.

## Scheduled updates

To opt in, set these variables in each selected node's server environment:

```sh
GAH_ENABLE_ADMIN_UPDATE=1
GAH_AUTO_UPDATE_AGENT_CLIS=1
```

Restart that node's server after changing its environment. CLI auto-update is off by default and independent of the node's GAH auto-update preference.

Central checks CLI updates on the existing five-minute fleet update timer. Each node validates its own opt-in and admin policy.

Each tick updates at most one outdated CLI per node. Unattempted CLIs go first, so one failing installer cannot block the others.

The next tick retries deferred work. Registry failures do not authorize an update.

After success, GAH invalidates cached model connections and refreshes the existing gah-router inventory. Active chat prompts finish before their old processes close.

Vibe and OpenHands model readers already query their inventories on demand. Direct terminal updates also invalidate model connections when the server next resolves an adapter.

## Installation limits

Codex uses npm for npm installations and Homebrew for cask installations. Native installations require the `update` subcommand.

Vibe supports uv and Homebrew installations. Other installation methods can fail without changing the reported installed version.

A package registry can publish a version before a native updater makes it available. The Agents page continues to show that version difference.
