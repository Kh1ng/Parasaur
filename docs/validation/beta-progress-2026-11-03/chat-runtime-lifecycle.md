# Worker execution and archived chat runtimes

The Windows/WSL worker now has native Linux Codex 0.166.0, matching the installed
Windows CLI. Its executable is pinned in the existing gah profile. The existing
Windows ChatGPT login cache was transferred privately into the previously empty
Linux cache with mode 0600, following the official
[headless authentication fallback](https://learn.chatgpt.com/docs/auth#fallback-authenticate-locally-and-copy-your-auth-cache).
No credential was printed or committed. This is a transferred login, not a new
shared credential/refresh service. No routing model, review policy or claim
ownership changed. The original profile is backed up locally.

Checks under the worker service's actual environment resolve Codex, recognize
the ChatGPT login, and report Codex configured. Central also observes the login
as OK. The worker's configured LM Studio endpoint is off, and its Windows model
inventory contains only an embedding model, not either configured coding model.
The LM Studio auth probe's OK state does not establish endpoint/model readiness.
No model download or LM Studio server start was performed.

## Confirmed lifecycle defect

A real request through the live worker chat router completed using the backend's
selected `gpt-6-astra` model and returned the expected diagnostic reply. No tools
or permissions were used. Its disposable worktree archived successfully, but
the bridge and Codex processes remained alive afterward. Catalog discovery also
retained its separate runtime. These are process-retention observations, not
proof of the Linux controller or Mac client memory reports' root cause.

Public `256276a4` adds per-conversation runtime release to archive on both central
and worker. The registry releases the matching key across default and named
account adapters, including backends used before a handoff. ACP startup is
aborted before it can publish a late child; an active prompt prevents release.
Idle ACP process trees are terminated and their exit confirmed before archive
can remove the workspace. Other conversations retain their runtimes. Branches,
dirty-work patches and durable history keep their existing archive behavior.

## Validation and deployment

- Sixteen focused Windows tests and 21 Linux ACP/worker/session tests pass.
  A real descendant-process fixture verifies complete exit, sibling isolation,
  runtime recreation on restore, late-start cancellation and active-turn refusal.
- The public full run had one failure from an omitted nested cached dependency;
  after linking that verified dependency directory, its 27-test file passes.
- Private assembly `2bd0d41`, pinned to `256276a4`, passes 1,112 server tests
  (38 explicit skips), 16 mock cases, production builds and private
  artifact/input/credential-transport checks. Log:
  `/home/khing/.cache/parasaur-enterprise-current.mek3v9qt/build.log`.
- Four compiled modules are deployed to the Windows worker and central API.
  Their original hashes match the reproducible base build; dependency locks
  are unchanged. Backups and automatic code rollback are retained. The central
  controller, legacy handoff, configuration, credentials, chat index and real
  notifications are preserved. No Mac files were changed.
- Three live worker create/catalog/archive cycles pass. Each observes three new
  processes, confirms all exit before archive returns, and leaves no owned
  processes. These checks send no further model prompts. The worker has no
  remaining child processes after validation.

Worker PID is 140562; central API PID is 578182. The central controller retains
PID 437234. After restart, central served HTTP while provider readiness was
pending; its status-reader CLI was observed in disk sleep. That process later
exited, and TLS health reports healthy/provider-registry ready at 08:20 UTC on
2026-11-06. Cold storage latency is not fixed by this lifecycle change.

Metadata-only reports are in ignored `artifacts/central-live-20261105/`, including
`worker-codex-live-smoke.json`, `worker-codex-service-readiness.json`,
`execution-fleet-codex-ready.json`, `worker-chat-release-deployment.json`,
`worker-chat-release-live-validation.json` and `chat-release-api-deployment.json`.
The reproducible patch manifest is in `artifacts/parasaur-chat-release-256276a4/`.

Full factory progression, exact factory routing/review readiness, provider
billing reconciliation, GitLab login/proxy continuity and Mac memory acceptance
remain open. The latest fleet GitHub probe reports error, which needs a fresh
successful observation before claiming Git transport readiness. This pass does
not close either memory issue or the broader usage/auth reports.
