# Windows native output acceptance — 2026-11-04

Verified public `f322996bc2e147e97159a46aa86147e037a973a0` and private
`d531a72fa3c72b61924eb8261174be5257765773`. The existing shared download
path works with this machine's native WebView2 filesystem APIs; no new desktop
filesystem bridge, plugin or remote permission was added.

## Observed native behavior

An opt-in hidden Tauri fixture used an isolated Windows browser profile and
the actual shared `ComputeOutputSection`. Its actual owner-authenticated API
served a disposable **20 MiB + 37-byte** archive from WSL. The installed
WebView2 engine reported a secure context and an available `showSaveFilePicker`.
Its user agent identifies Edge/Chromium 156.

Only the interactive picker was substituted: it returned a real origin-private
`FileSystemFileHandle`. Its writer, writes, commit, abort and subsequent file
reads were native browser filesystem operations, rather than mocked methods.

- Success saved exactly **20,971,557 bytes**, using twenty 1 MiB writes and one
  37-byte write. The saved SHA-256 was
  `04734ab925d70b1941771de0591161043e90eaa76fa179040349c8da7b4f51c2`.
- Corrupting the second downloaded block caused rejection after one verified
  write. The real writer aborted and the existing destination remained unchanged.
- Canceling after the first write aborted the real writer and preserved that
  same existing destination.
- The server independently confirmed authenticated API requests and its
  unchanged retained archive: three verification plans and 25 block requests.

The initial cancellation fixture canceled before writing began; its one-abort
expectation therefore failed. It now waits for the new operation's first write
and counts abort only after the actual filesystem abort resolves. The final
check passes without changing production download behavior.

## Repeatable checks and evidence

The committed [PowerShell runner](../../../scripts/test-native-output.ps1)
checks the live fixture's instance ID before launching the ignored native test,
retains browser/server evidence, restores its environment and stops its fixture
server. A stale ready-file instance was explicitly rejected before native launch;
the correct live server remained available for the subsequent successful run.
The private repository documents the Linux server and Windows runner workflow
in `docs/native-output-validation.md`.

Retained production assembly:
`/home/khing/.cache/parasaur-enterprise-current.28ukd4zr/source`.
Its sibling `build.log` records **1,072 server cases passed**, 37 explicit skips,
**14 sample cases passed**, typechecks, production builds and artifact/transport
guards. No fixture markers appear in the production web output.

Windows desktop tests passed **35 ordinary cases** with seven explicit ignores.
The separate opt-in hidden test passed all three native filesystem scenarios.
Its final evidence is retained locally under
`artifacts/native-output/dbd4c537-e16d-4390-9646-db7ac0412c20/`
as `windows.log` and `native-output-result.json`. The assembled fixture also
retains its JSON under `.build/native-output-fixture/`.

This increment adds test drivers and documentation. Scoped Git diffs confirm
that public CLI/Cargo/server/web/contracts and private production server/web
sources match the preceding [saved-output controls](saved-output-controls.md)
pair. Its 77 dedicated isolation cases and real Auth/PostgREST/relay evidence
remain the prior verification of those unchanged components; they were not
repeated for this test-only increment.

The fixture server completed shutdown and no fixture WebView2 process remained.
Some browser profile cache files remained locked during the test's immediate
cleanup. Automatic approval review rejected subsequent cleanup with the reason
“blocked by policy”; those disposable Temp profiles were left in place.

## Remaining acceptance

The interactive save dialog, user-selected Windows filesystem paths, the
installed application and signed release/update acceptance remain unverified.
The test opens no visible window or dialog, reads no installed configuration and
uses no real provider or factory worker. No installed application or live service
was updated. Journal retention and orphan capability-metadata maintenance remain
unfinished; freeing an archive slot does not clear the journal limit.
