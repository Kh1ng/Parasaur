# GitLab file hook ingestion

The central server accepts JSON at `POST /api/integrations/gitlab/events`.
Forward the raw file-hook payload with `Content-Type: application/json` and
`X-Parasaur-Hook-Secret`. Configure `GAH_GITLAB_HOOK_SECRET` in the server's
private environment, never in committed configuration. Forward
`X-Gitlab-Event-UUID` when available; otherwise the payload hash identifies a
redelivery.

Set `GAH_GITLAB_HOOK_PROJECTS` to a JSON array of project bindings, for example:

```json
[{"projectId":42,"profile":"project","repo":"group/repo","apiBase":"https://gitlab.example/api/v4","bot":"parasaur","providerKind":"codex","instanceId":"codex-0"}]
```

The project ID must belong to the configured GitLab instance. `providerKind`
and `instanceId` select the worker backend, independently of the GitLab source.
Set `GITLAB_TOKEN` for GitLab user lookup and queued comments. Set
`GAH_PANEL_URL` to the operator-visible panel URL. Use HTTPS when forwarding
outside the local host.

The existing profile publishing fields `trusted_issue_human_authors` and
`trusted_issue_bot_authors` govern actor trust. Missing or unknown identities
fail closed; the user API establishes whether the actor is a bot. Upgrade the
CLI together with the server so config-show exposes these allowlists.

New bot assignments, newly requested bot reviews, and new issue/MR notes
mentioning `@parasaur` queue work. The note text becomes the worker instruction;
issue descriptions do not configure trust or routing. Other events, including
pipelines, are ignored. Pipeline dispatch is not enabled.

Private receipts under `~/.local/share/gah/sessions/gitlab-events` reserve each
event before acknowledgment. Dispatch and comments run asynchronously. A
redelivery does not launch another run, including after server restart. If a
process stops between reservation and dispatch, or dispatch has an uncertain
outcome, inspect the receipt and session artifacts before manually retrying.
`needs_review` records uncertainty; `queued_comment_failed` preserves the run
ID when only notification failed. No automatic retry risks a second dispatch.

Compatibility tests use constructed, sanitized GitLab 19.x payload shapes in
`apps/server/src/fixtures/gitlab-19`; live recorded payloads were not supplied.
