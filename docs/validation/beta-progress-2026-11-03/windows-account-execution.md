# Account execution and rebind attribution

The real Windows-to-WSL regression now launches two configured Codex instances
concurrently through `gah config exec-backend-instance`. Its synthetic runner
checks the selected key, isolated HOME, explicit OpenAI provider configuration
and removal of an unrelated ambient key. It returns only account labels and
state paths. Each instance writes its own synthetic session marker.

Rebinding one instance to the other credential changes the next launched key.
Removing the remaining credential then prevents either instance from starting:
both launches fail and neither recreates its deleted session marker. There is
no fallback to the ambient key. No provider CLI or network request is involved.

Inspection also found that rebinding changed the key while retaining the old
safe account label. Durable identity deliberately omits credential IDs, so this
left subsequent attribution describing the previous account. The CLI now accepts
optional `--account-label` on `config set-backend-instance-credential`, validating
and saving it together with the binding. Native account selection supplies the
selected credential's logical ID as the safe label. Calls without the new option
retain their operator-defined labels for compatibility; existing history is not
rewritten. Credential values and local credential-source fields remain excluded
from durable identity.

The core regression checks the serialized execution identity, compatibility
without the option, and that an invalid label or missing credential leaves the
saved configuration unchanged. All 23 credential-binding CLI tests passed under
WSL. Core Clippy passed for all targets/features with warnings denied. The native
suite passed 35 cases with two fixture skips; the real WSL fixture was explicitly
run and passed against the updated CLI. Its initial execution check incorrectly
expected a Codex setting in an environment string; it now checks the actual
configuration argument and selected provider, as the production launcher uses.

The expanded Windows check creates an isolated Linux home after the WSL exec
boundary and uses a fake Codex executable. It proves account selection and
revocation at that launcher boundary. It does not prove real provider login,
model responses, metered usage ingestion, billed spend or full factory dispatch.
One failed-run synthetic fixture from this pass remains in `/tmp`; the later
successful runs cleaned up their own fixtures.

The new native rebind command requires the updated CLI. An older CLI rejects the
new option before changing the binding; it cannot silently retain stale labels.
Desktop and worker release artifacts must be delivered from the same revision.
