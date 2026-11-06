# Configured reviewer on the Windows worker

At 2026-11-06T08:23Z the effective gah profile on central has implementation
and review candidates, a routine reviewer and escalation requirements. The
Windows worker's effective profile has empty candidate lists, no routine
reviewer and no escalation list. Registering the same profile name did not
transfer that policy. Central's Rust controller still executes locally; its
Node fleet coordinator handles separately submitted sessions. Starting a
second controller with the worker's empty policy would not reproduce central's
configured review process.

Central's routine reviewer is OpenCode model
`tak-mistral-vibe/zai-glm-5-3`. Its provider points at Mistral's `/v1` API and
uses an existing node-local credential-file reference. An authenticated model
inventory request returned 200 and included the exact configured model.
The worker initially configured only LM Studio, whose coding models and running
endpoint are absent. A successful local-provider auth probe did not establish
review readiness.

## Provisioning and execution evidence

Only the configured review provider was added to Windows/WSL OpenCode. Its
existing account credential traveled through SSH and stdin, never through a
Windows temporary file or diagnostic output. The destination key and backup
use mode 0600; OpenCode references the key file. Existing OpenCode settings,
including the LM Studio default, are preserved. This provisions a node-local
copy; future central credential rotation is not automatically synchronized.
The credential's observed format does not establish its billing class or
expiry. No new login was performed.

The worker independently queried the provider model inventory: HTTP 200, exact
model present. A disposable chat through the production worker router then
selected `tak-mistral-vibe/zai-glm-5-3` and received the expected test reply.
The reported model matched the requested model. No tools or permissions were
used. The workspace archived successfully, and the worker's process tree was
empty afterward. Recorded cost remains unknown. The returned token fields
are preserved without inventing a bill or reconciling their cache breakdown.

Metadata-only reports are in ignored
`artifacts/central-live-20261105/worker-review-provider-models.json` and
`worker-review-provider-live-smoke.json`. The original OpenCode config backup
is `~/.local/state/gah/worker-recovery-20261106/opencode-before-review-provider.jsonc`.
Neither that backup nor the credential is a repository artifact.

## Remaining factory gates

The worker's factory policy was not replaced, and no factory loop or real
issue/PR dispatch was started. Central claims are configured on the worker,
while central's current `registry_central_url` is unset. Claim participation
must be verified across all executing controllers before simultaneous loops
can be treated as coordinated. Assignment alone is not a claim or execution
receipt.

Next work must establish how central's execution/review requirements reach
the destination while retaining node-local paths and credentials, preserve
the configured review/escalation requirements and central claim arbitration,
and validate claim-to-validation-to-review progression. Provider connectivity
alone does not prove any of those requirements. Production publication and
the existing GitLab authentication/proxy gates remain separate.
