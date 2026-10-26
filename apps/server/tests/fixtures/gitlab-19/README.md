# GitLab 19.0 event examples

Source: [GitLab v19.0.0-ee webhook event examples](https://gitlab.com/gitlab-org/gitlab/-/blob/v19.0.0-ee/doc/user/project/integrations/webhook_events.md).

These fixtures retain the documented payload fields for issues, merge requests, notes, and pipelines.
They are documentation examples, not captures from a production server.
Unrelated labels, builds, and repository metadata are omitted.
Hosts, projects, users, IDs, timestamps, and task text use example values.
Assignment, review-request, mention, and failure values exercise the endpoint triggers.
No event UUID header is present, as with the companion file hook.

Live-capture replacement is pending owner access to a self-hosted GitLab 19.x
instance emitting through `parasaur-forward`. Capture one qualifying issue,
merge request, note, and pipeline delivery; redact secrets, personal data,
private hosts and project text while preserving payload structure and trigger
fields. Record the emitting GitLab version and capture provenance here when
replacing these examples. Do not label generated or documented examples as live
captures.
