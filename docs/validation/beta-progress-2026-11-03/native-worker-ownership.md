# Native handoff ownership and canonical acknowledgement

This increment adds the central ownership boundary for native handoff. It is
source-only: no live claim, controller, API or installed CLI was changed.
It does not authorize a worker launch. Fresh authority, actual-host admission,
the durable per-run offer/executor journal and automatic reconciliation remain
unfinished.

## Ownership while execution is uncertain

`ClaimsService.reserveNative` atomically reserves work for one coordinator,
native run UUID, executing node and prepared-context digest. It refuses to
adopt any existing legacy lease, including the same coordinator's lease. Exact
replays preserve timestamps/expiry and re-establish file/directory durability
before acknowledgement. Changed identity/context and reused active run IDs
conflict. The context digest binds the separately prepared request; reserving
it grants no execution permission.

The initial phase is `reserved`. A broker can persist an `offered` intent
before transport, then record observed `running` or `reconciling` state. A
later state cannot be reset to an offer. Ordinary acquire, renewal, release
and lease expiry cannot remove this hold. Expired native holds remain visible
with their run, worker and phase, and conflict messages identify reconciliation
as the waiting condition. The expiry remains an authority deadline, not proof
that an agent exited.

Issue aliases follow Rust's `work_claim::normalize_work_identity`; `#1525`,
`1525` and `TICKET-001525` share ownership. Tuple keys avoid delimiter
collisions. Duplicate canonical identities and malformed native holds refuse
store loading. Returned objects are copies, so observers cannot mutate a
reservation without persistence.

The existing deployment contract still requires one central claims-service
writer. Active ownership is bounded by the actual unresolved work; this does
not add a permanent in-memory run-history map. The broker's per-run journal
must retain terminal receipts and refuse re-offering completed runs.

## Durable persistence and rollback

The existing atomic-file writer now has a required directory-sync mode.
Claims publish their new in-memory state only after the private file and, on
Linux, its containing directory have been synchronized. Windows uses the
writer's writable file handle for `FlushFileBuffers`; directory synchronization
is not supported through this API on Windows. Actual native execution remains
unsupported on native Windows; this pass tests ownership semantics there.

Any persistence error makes that service instance refuse further operations.
A post-rename failure can have committed new disk state, so returning to the
old in-memory map would be unsafe. Tests inject that exact directory-sync
failure and verify reload preserves the uncertain commit and older hold.

While any native hold exists, the store uses version 2 with `fenced_leases`
instead of the legacy `leases` array. Older APIs reject it before granting
claims. After canonical completion removes the final native hold, the store
returns to the legacy shape while retaining unrelated leases. This prevents a
rollback from silently expiring an unresolved native reservation.

## Canonical result acknowledgement

`gah ledger receive-dispatch --acknowledgement` emits a structured receipt only
after the existing receiver synchronizes canonical append or identical replay.
It includes outcome, native run, node, profile, work identity, context digest
and original receipt digest. Existing callers without the flag retain the
original JSON-string outcome. The operation remains local-only and
security-sensitive in the capability manifest.

`runReceiveNativeResult` is trusted central broker plumbing with bounded JSON
output and fixed argv. Its binding and receipt paths must be server-owned
artifacts; there is no new generic remote execution or result-import endpoint.
`ClaimsService.completeNative` persists `result_pending` before awaiting this
importer. Import failures and mismatched acknowledgements keep ownership. A
matching canonical acknowledgement releases only that exact current fence.
Concurrent or late replay cannot release newer work.

A cancellation request alone now fails native receipt validation. Both natural
exit and cancellation require an observed exit code or signal. This closes a
receiver exception that could otherwise acknowledge a cancel request with no
process-death evidence. A confirmed cancellation retains its signal and does
not become job success.

## Verification

- Windows: 25 atomic/claim cases pass, two POSIX-specific cases explicitly skip.
- Linux: all 24 claim cases pass, including post-rename sync failure, held
  ownership during awaited import, failed reconciliation, rollback refusal,
  service reload, exact replay and preservation of a newer reservation.
- Rust: 136 ledger cases, nine capability cases and seven CLI-ledger cases
  pass; the CLI cases include shared fixture checks. Formatting and
  all-target/all-feature Clippy with warnings denied pass. Server/contracts
  and shared-web typechecking pass.
- A disposable WSL bridge uses the actual compiled Rust receiver and actual
  server import wrapper. Unconfirmed exit and cancellation are refused without
  changing history or releasing the hold. A failed job is durably recorded as
  failed with unknown cost preserved, then its exact acknowledgement releases
  ownership. Replay adds no row and preserves a newer hold. The provider
  executable canary and session directory remain absent. No model, real issue
  or production service participates.

Rust log:
`/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-claim-import-checks.log`.
Ignored metadata: `artifacts/central-live-20261105/native-claim-import-bridge.json`.
The bridge records hashes of the typechecked TypeScript sources it executes.
Windows/Linux focused logs are `artifacts/native-claims-windows-tests.log` and
`artifacts/native-claims-linux-tests.log`.

## Matched release verification

Public `cd87332e` was built from a fresh immutable archive with all 606
Rust/Cargo inputs checked. Artifact:
`artifacts/parasaur-native-context-cd87332e/`; optimized binary SHA-256:
`ad102428bfb110449770e7f5ace5efb88d2e4df8621791e1adfda8aef9cd0b68`.
It passes native context and zero-dollar spend refusal production smokes. The
same Rust/server acknowledgement bridge also passes using this exact optimized
binary, including unconfirmed-cancellation refusal and receipt replay. It
remains staged, not installed. Build log:
`/home/khing/.cache/parasaur-native-context-release.jykycq1w/build.log`.

Matched private `910d660` pins public `cd87332e` and passes 1,170 server
cases (38 explicit skips), 17 mocks, production server/shared-web builds and
artifact/input/credential/transport guards. Its validation Supabase values are
fixtures, not live SSO evidence. Log:
`/home/khing/.cache/parasaur-enterprise-current.1ui_o51i/build.log`.

The existing controller observation remains running at 463 samples over
28,005.797 seconds, with zero gaps and zero OOM kills. Controller 666974 and
observer 667504 are unchanged. Ten-hour and sustained-load acceptance remain
open. Metadata is retained in ignored
`artifacts/central-live-20261105/native-ownership-memory.json`.

The next broker increment must persist per-run offers before transport, retain
the original central profile/work claims, refresh execution authority and
permissions, and obtain capacity on the actual execution host. Its completion
path must use the separately prepared binding and canonical acknowledgement
above. Neither these fixture results nor an observation timeout permits
re-offering an uncertain run or starting an empty-history controller.
