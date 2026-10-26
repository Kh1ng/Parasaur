# GitLab event ingestion

This endpoint implements the events section of the [companion contract](https://github.com/Kh1ng/parasaur-gitlab/blob/main/docs/parasaur-contract.md).
The companion repository is private. The integration epic is [#1591](https://github.com/Kh1ng/git-agent-harness/issues/1591).

## Configuration

Set these environment variables on the central server:

- `GAH_GITLAB_HOOK_SECRET`: required shared secret. Use the file hook's `PARASAUR_HOOK_SECRET` value. Never put this secret in committed configuration.
- `GAH_GITLAB_BOT_USERNAME`: bot username. Default: `parasaur`.
- `GAH_GITLAB_PIPELINE_REPAIR`: set `1` to enable failed-pipeline repair. Default: off.
- `GAH_GITLAB_PANEL_URL`: required public dashboard base URL, including any path prefix. Example: `https://panel.example.com/parasaur/`.
- `GAH_GITLAB_EVENTS_RATE_LIMIT`: positive integer requests per minute per source IP. Default: `600`, also used for invalid values. Applies before secret verification and event classification, including unsupported events and invalid secrets.
- `GAH_GITLAB_EVENTS_STORE_PATH`: optional receipt-file path. Default: `$XDG_STATE_HOME/gah/gitlab-events.json`, or `~/.local/state/gah/gitlab-events.json`.

Configure a GitLab profile with `provider`, `repo`, `provider_api_base`, and `provider_project_id`.
Authenticate `glab` as the configured bot with the existing provider credentials.
The profile must match the event's project URL and repository path exactly.
Ambiguous profiles fail closed.

Use the existing profile policy:

```toml
[profiles.example.publishing]
trusted_issue_human_authors = ["maintainer"]
trusted_issue_bot_authors = ["parasaur"]
allow_issue_comments = true
```

GitLab has no implicit trusted owner. Empty or absent author lists grant no trust.
The hook uses `gah config show --json --full` to read these lists.
Older CLIs without `event_policy` cannot authorize event dispatch.
Comment policy and `delivery_mode = "handoff"` still apply.

## Request and response

Send `POST /api/integrations/gitlab/events` with `Content-Type: application/json`.
Send the shared secret in `X-Parasaur-Hook-Secret`.
The server compares fixed-size secret hashes in constant time.
The body is raw GitLab JSON, without an envelope.
No dashboard bearer token is required for this route.
Workers do not expose this integration.
Remote requests require HTTPS unless the existing insecure-HTTP override is enabled.
The standard JSON body limit is 100 KiB.

The companion file hook forwards stdin unchanged. It sends no event UUID header.
Its curl command has a ten-second timeout. It has no retry loop and does not fail on HTTP error status.
Use hook and server logs to diagnose rejected deliveries.

- `202`: the server saved a receipt and scheduled asynchronous authorization and dispatch.
- `204`: an unsupported event, an inactive trigger, or a duplicate receipt.
- `401`: the secret is absent, incorrect, or unconfigured.
- `429`: the per-source-IP request limit was exceeded. Default: 600 requests per minute.
- `503`: panel URL unavailable, receipt storage unavailable or full, or 64 asynchronous deliveries already pending.

Invalid JSON returns `400`. Oversized JSON returns `413`.
Authorization can reject a candidate after `202`; this starts no worker.
Provider failures produce a server log entry without provider output or credentials.

## Dispatch rules

The server loads the actor through `glab` and checks the existing human or bot author list.
It verifies the configured project ID through the provider API.
Issue assignment also requires a trusted issue author.
Issue bodies never select hook configuration, authorization, or dispatch modes.
Rust retains its issue intake, label, claims, routing, and publication checks.
Managed issues retain the existing dashboard dispatch fence.

- Issue assignment: an `open` event assigned to the configured bot, or an `update` that adds the bot. Dispatch mode: `improve`, target: `#<iid>`.
- MR review request: an `open` event with the bot reviewer, a reviewer addition, or a review re-request. Dispatch mode: `review`, MR: `<iid>`.
- Issue or MR note: a whole `@parasaur` mention in a non-system note. The note text is the worker instruction. Issue mode: `improve`. MR mode: `fix`.
- Failed pipeline: an enabled, failed branch pipeline with an open MR authored by the configured bot. The MR must match the project and pipeline SHA. Its branch must start with `gah/<repo_id>-`. Dispatch mode: `fix` on that MR.

Bot-authored mention notes are ignored. Commit and snippet notes are ignored.
Unrelated assignment or reviewer edits do not dispatch.
Pipeline events without an open bot-owned MR are ignored.
All starts use the same fleet coordinator as `POST /api/dispatch`, then the existing `gah dispatch` path.
A successful start posts `Parasaur queued a run` with a dashboard session link.
A comment failure never starts another run. This integration does not merge MRs.

## Redelivery and limits

If supplied, `X-Gitlab-Event-UUID` or body `event_uuid` identifies the event.
Otherwise, receipts use project, object kind, object ID, and action or pipeline status.
Issue and MR receipts also include revision and assignment changes. Note receipts include revision and note text.
The file stores only receipt hashes and expiry times, with private permissions and atomic replacement.
One central server owns the receipt file. Do not share it between server processes.
Receipts survive restart and expire after 24 hours. The store holds at most 10,000 unexpired receipts.
A full store refuses new events instead of removing live receipts.

The receipt is saved before `202`, so replay cannot repeat a dispatch after a comment failure or restart.
There is no durable work queue or automatic dispatch retry.
A crash after acknowledgement can leave a receipt without a run.
Before manual replay, check the run state and remove only the affected receipt hash from the file with the server stopped.

Fixtures under `apps/server/tests/fixtures/gitlab-19/` derive from GitLab 19.0 documentation examples.
They are sanitized examples, not live captures. Validate the deployment with real file-hook deliveries before enabling unattended work.
