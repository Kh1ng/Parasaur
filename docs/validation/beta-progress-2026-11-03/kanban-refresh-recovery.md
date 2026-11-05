# Factory observations after a failed refresh — 2026-11-05

A failed status refresh retained the old snapshot without a warning on Kanban.
Its worker cards and navbar still claimed execution was live, while available
capacity could appear confirmed. Starting a retry also cleared the stored error
before the request succeeded.

The existing status resource now retains its failed-read error through retries
for the same profile and clears it on success. Switching profiles still starts
with that profile's own empty resource. No second freshness store is introduced.

Kanban preserves recorded work, gate explanations and timestamps, labels the
board as last recorded, and treats execution and capacity as unconfirmed. It
does not invent a stale heartbeat or conclude a process stopped. Assignment is
disabled until a successful status read. The navbar keeps recorded job details
under an unavailable label instead of advertising active work or an idle factory.
The app also scopes retained status to the selected profile. A successful refresh
restores the current board and activity labels.

The alternative was to clear all cached status on failure. Keeping the snapshot
with explicit uncertainty preserves the machine, work and gate context needed
to investigate an inaccessible worker without presenting it as current evidence.

## Windows validation

- Kanban projection and status-store unit suites: 35 passed.
- Existing Kanban component suite: 13 passed.
- Full-browser Kanban failure/retry/recovery and Overview diagnostics: two
  passed against isolated sample services. The new flow also passed separately
  after adding observation of the actual outbound session-start socket messages.
- Web typecheck and production build passed; `git diff --check` passed.
- Desktop and 390px failure-state screenshots were inspected. The warning wraps
  within the mobile viewport and recorded work remains inspectable.

The browser flow verifies running work first, then an HTTP 502, retained job
context, no live running indicator, unknown capacity, disabled assignment,
unavailable navbar details, the same warning during a deliberately held retry,
and restored running state only after that retry succeeds. No session-start
message is sent. The store check separately asserts warning retention during
the pending request. The isolated test listeners were confirmed stopped afterward.

Screenshots remain under `apps/web/test-results/e2e/` in the
`kanban-recovery-failed-and-597fa-out-claiming-live-execution-chromium` result
directory. This verifies controlled read failure, not live worker recovery,
real provider execution or installed desktop acceptance.

## Assembled integration

Private `e630a95597b316ea1798dc671fc9fffe87897217` pins public
`191168dcefa07f554456aeede1b438ffc9d0cc9d`. The full combined build passed
1,099 server cases (38 explicit skips), 15 mock cases, typechecks, production
builds and private artifact/input/transport guards. All 35 focused board/store
checks also passed in the actual assembled source. The private App retains
the profile-scoped failure wiring; the built dashboard contains the unavailable
activity, last-recorded job and assignment guidance. The release manifest binds
the exact pair.

Evidence is retained under
`/home/khing/.cache/parasaur-enterprise-current.t791nclq/`: `build.log`,
`board-store-verification.log`, and `source/.build/release.json`.
This increment changes only public UI/store code and the private core pin.
Native isolation and real Auth evidence belong to their earlier verified pair;
no fresh native/Auth run is claimed. Installed binaries, the public recovery ZIP,
live claims, provider accounts and remote services are unchanged. No push or
deployment occurred. Authenticated live pipeline and installed-app acceptance
remain required; the overall beta goal is unfinished.
