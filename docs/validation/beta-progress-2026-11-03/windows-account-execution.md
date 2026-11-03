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

## Paired candidate and local installation

Both optimized builds completed from immutable source
`b738b9d2820770c3aee3e23fae71a8dd1fe76b43`. The complete Windows-to-WSL fixture
passed again against that optimized CLI. The paired desktop/worker manifests
passed the actual packaged Windows installer's validator for all five routes.
The ignored delivery artifact is `artifacts/parasaur-windows-beta-b738b9d2.zip`,
SHA-256 `aa53e2e572122c022d9d1d939ddec6987cd7b6de48f03b50fcea702002e139c3`.
The ZIP contains the combined bundle, both manifests and the public source
archive. Private submodule contents and user/test profiles are not in it.

The saved desktop distribution is Ubuntu-24.04. Read-only inspection confirmed
its user had no `~/.cargo/bin/gah`. The verified CLI was installed there with
mode 755; version and credential-list commands passed (no saved accounts).
This installs only a user CLI: no setup wizard, daemon, worker loop or service
was run. Its SHA-256 is
`ce84046e95b5e47a6a253eeb36df95c256732cd6c4bc8b115c52fb0ac0efe3ee`.
The binary requires glibc 2.39; this does not establish older-Linux compatibility.

The matching current-user desktop installer returned zero and preserved the
saved settings hash. Installed bytes match the optimized executable with only
Tauri's expected `UNK` to `NSS` bundle marker patch; installed SHA-256 is
`b70ca858f0c56c1a49f778d62082ae76432b2adddb5067a374c8987dc44c9139`.
The executable was launched by its exact installed path.
Process inspection confirmed that path after launch. The final UI observation
reported a minimized window; it was left minimized rather than counted as a
successful rendering check. Earlier recovery rendering evidence remains separate.

A separate opt-in read-only native test now invokes the production credential
path against the actual saved Windows settings and installed WSL CLI, without
injecting an isolated home. It passed and parsed the account metadata contract.
No user account was created or changed by that check. The final native suite
passed 35 cases with three skipped fixtures; both opt-in WSL checks were run
explicitly and passed. These artifacts are unsigned and unpublished, and the
live central server has not been changed.
