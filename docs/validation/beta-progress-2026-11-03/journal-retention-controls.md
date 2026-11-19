# Execution receipt retention controls — 2026-11-04

The candidate pairs private `5ef4458` with public core
`189f01e3f7be11b89e0fa86361cc8688b9ce1970`. Settings → General → Saved output
now exposes the journal admission limit separately from archive storage. A full
journal explains why new compute jobs are paused. Unavailable counts remain
unknown. This is local Windows/WSL candidate evidence, not hosted acceptance.

## Maintenance behavior

An owner can review up to 50 delivered receipts and explicitly confirm removal.
Every receipt needs fresh cloud proof that its exact workspace/node/work/claim
cannot execute again, verified local ownership and stopped broker evidence, and
no retained storage binding. Pending delivery, uncertain ownership, unreadable
or unbound storage, unfamiliar files and unavailable cloud proof retain data.
Archive removal and receipt removal are independent operations.

Removal first writes a private, durable checkpoint of the original receipt.
A new reviewed pass can resume after partial unlinking or after the record
directory is gone. Lookup retains the checkpoint's delivery evidence so empty
storage recovery does not mistake interrupted cleanup for an absent result.
An unfinished checkpoint blocks a new local execution of that claim.

The checkpoint bound is 64 KiB, allowing all eight original 4 KiB phase files
after base64 encoding. A regression reproduced the earlier reader-bound failure
using valid padded JSON and passes after the repair. Original phase read limits
are unchanged. One failed cleanup does not stop unrelated eligible receipts.

The operation uses the owner-only API boundary, explicit confirmation, durable
audit/idempotency protection, exact review revisions and a 20-second cleanup
budget. An uncertain HTTP outcome requires refresh and another review. Switching
machines invalidates pending reviews. Read-only verified archive download remains
available with an unreadable journal; removal remains blocked.

The private migration `20261104000004_compute_retirement_proof.sql` supplies the
read-only exact-claim proof. It requires an authorized enrolled device and returns
true only for a terminal or superseded claim. Missing work and a changed device
alone do not prove retirement. This migration has not been deployed.

## Verification

On the final committed private `5ef4458392eb349c4d13ea68ee3f7ad1bd27ec76` /
public `189f01e3` pair, retained at
`/home/khing/.cache/parasaur-enterprise-current.mo6zeeoz/source`:

- Combined production build: **1,084 server cases passed**, 37 explicit skips,
  zero failures; **15 mock cases passed**. Server/web typechecks, production
  builds, input/artifact guards and transformed browser transport checks passed.
- Disposable SQL migration replay and authorization checks passed, including
  active-claim denial, released/terminal claim proof, wrong workspace/node,
  missing work and revoked enrollment.
- Real Auth/PostgREST maintenance removed one eligible delivered receipt after
  separate archive discard, while preserving the other archive and receipt.
  The old claim's start/renew request was denied. Attempting its program again
  through the actual native isolation path produced no additional provider call
  or archive and could not overwrite the cloud completion result.
- Real automatic queue/inference/archive, lost start/result replies, consumer
  kill/restart, empty-storage recovery, inference retry receipts, HTTP/WebSocket
  relay, JWT/edge authorization, recipient isolation, entitlement and removal
  checks all passed. All fixture providers were loopback synthetic services.

Final logs are `build.log` and `auth-verification.log` beside this checkout.
The final user-manager check found no remaining `parasaur-job-*.scope` units.

On private `a6566fc` / public `189f01e3`, retained at
`/home/khing/.cache/parasaur-enterprise-current._f2lvx51/source`:

- Dedicated isolation: **89 passed**, one platform-only skip, zero failures.
  This includes worker admission resuming after confirmed maintenance, retained
  uncertain receipts, changed storage during proof, bounded review, cancellation,
  partial-removal restart and checkpoint-backed empty-slot recovery.
- Assembled shared UI: **18 passed**, including 17 behavior cases and one
  desktop/390px render/focus case. All four final captures were inspected.
- Full sample Settings workflow: **one passed** through the actual API proxy.
  Receipt cleanup changes 4 entries to 3 while preserving saved output; separate
  output discard leaves the receipt count unchanged; reset restores both.

An initial isolation invocation ran before native-host retention completed and
failed on missing files. The verified-binary rerun above passed. The first real
Auth run exposed a missing `readFileSync` import in the new acceptance driver;
`5ef4458` fixes only that import. Production sources, SQL and browser tests are
identical between these two private revisions.

The retained debug native host has SHA-256
`ac62e3cc98730f2e8839d7d79b867933e342b95b18a5f3f9cfd0708edaa1cf2e`.
CLI Rust/Cargo sources were matched to the pinned core before retaining it.
Sibling logs are `build.log`, `isolation-verification.log`,
`output-browser-verification.log` and `output-settings-verification.log`.

## Remaining acceptance

Unknown/legacy receipts, orphan capability metadata and pending delivery that
cannot reconcile with cloud state still require separate work. Fleet operators
also need admission pause reasons without access to another owner's private
archives. Installed desktop acceptance, hosted rollout and actual factory review
gates remain unverified. No live factory, provider account, installed application
or service was changed by these fixtures; MacBook work remains deferred.
