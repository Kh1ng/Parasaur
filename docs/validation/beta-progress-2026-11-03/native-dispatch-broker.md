# Durable native dispatch broker

This is source-only orchestration. `NativeDispatchBroker` has no production
constructor, HTTP route, permissive authorization fallback or worker executor.
Live services and installed CLIs were not changed. Fresh central policy/budget
authorization, actual-host reservations and the worker execution transport must
still be implemented and verified before it can dispatch real work.

The broker joins the existing context and claim boundaries with private
`<root>/<run UUID>/broker.json` journals. It validates the prepared manifest,
request, policy and full binding rather than accepting caller-supplied paths.
Only an explicitly selected backend and non-default model may enter this path.
Stage and start intents are recorded before their corresponding transport calls.
A network deadline aborts observation only; it never proves execution stopped.
Lost replies, worker absence, expiry, restart and stale ready observations do
not repeat either operation. A missing journal alongside held ownership refuses
recreation. Concurrent calls for one run serialize; completed promises leave the
in-memory map, while run identity remains on disk.

An idle host reservation and a fresh, exact, at-most-30-second start grant are
required. Expired/mismatched/denied grants cannot launch and release only the
idle reservation. Grants are passed to the transport and never stored in the
journal or returned view. They authorize the verified wrapper's initial start,
not subsequent backend attempts or publication. Production must implement those
later authority checks too.

Terminal observations prevent any later start. Receipts are privately spooled
before canonical import, with bounded input and at most four candidates per
run. Failed import retains the receipt and native ownership. A stale running
reply cannot postpone importing an already saved terminal receipt. Only the
canonical importer acknowledgement can release the exact matching ownership.
The spool hash protects original bytes; Rust's receipt digest identifies parsed
source JSON serialized canonically. These hashes need not be equal.

History chunks are reclaimed only after canonical acknowledgement. Cleanup
uses fixed, private, owned regular filenames, never recursive deletion. Failed
cleanup remains visible and terminal reconciliation retries it without starting
or importing again. Compact request, policy, binding, receipt and journal remain
for replay. Persistence failures latch the service until inspected/reloaded;
post-rename failures never restore an assumed previous state.

Disk journals reuse the existing atomic writer and claim service rather than
introducing a separate queue/database or retaining all run history in memory.
The deployment contract remains one central service writer per claims/journal
root. Operator deletion of durable history is not a supported replay mechanism.

## Verification on November 6

- Server `npm run --workspace=apps/server typecheck` passes.
- Windows: 37 of 39 combined broker/claim cases pass; two existing POSIX-only
  claim cases skip. All 15 broker cases pass.
- Ubuntu-24.04 WSL: all 39 combined cases pass, including all 15 broker cases.
  Cases cover concurrency, lost stage/start replies, deadlines, reload, stale
  state, failed imports, mismatched acknowledgements, missing journals, deferred
  cleanup and injected post-rename journal/receipt persistence failures.
- A disposable WSL bridge uses optimized Rust candidate `cd87332e` for actual
  context preparation, worker context checking and canonical import through
  `runReceiveNativeResult`. Its controlled transport loses the start reply,
  reloads the broker, then supplies a failed-process receipt. Stage, start and
  import each occur once; failure and unknown cost remain intact; the saved
  canonical digest matches the acknowledgement; ownership releases only after
  import; history is reclaimed; terminal replay starts/imports nothing again.
  The canary backend and real issues are never invoked.

Local logs: `artifacts/native-broker-windows-tests.log`,
`artifacts/native-broker-linux-tests.log` and
`artifacts/central-live-20261105/native-broker-bridge.json`. The bridge is run
with `artifacts/probe-native-broker.mjs` after
`node artifacts/build-native-claim-runtime.mjs`; it takes the staged Rust
binary as its sole argument. These are local validation artifacts, not a live
dispatch or beta-completion claim.
