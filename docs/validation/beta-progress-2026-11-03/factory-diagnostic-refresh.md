# Factory diagnostic refresh — Windows, 2026-11-04

Overview refreshed work and quota data without refreshing loop status. A loop
status failure became `false` in the view, displaying “Factory off” and offering
Start despite an unknown controller state. Last-decision failures disappeared;
an unscoped, late response could also replace another project's decision.

The existing Overview refresh path now includes both loop status and its last
decision, covering manual refresh, visibility refresh, the periodic tick and
WebSocket reconnect. Decisions use the existing profile-keyed resource pattern
in the shared store; pending resource identity prevents stale responses from
overwriting a newer project. The shared store was chosen over a second local
request/lifecycle implementation so diagnostics follow existing cache/error
semantics.

Unknown loop state displays “Factory status unavailable” and disables the
start/stop control. Errors give a visible Refresh recovery step. A failed
decision refresh retains the same project's prior evidence with an explicit
“refresh failed” label. Successful empty reads say no decision was recorded in
the last 24 hours, matching the server's event window. Both diagnostic API
reads have a 15-second abort deadline, including response-body consumption;
timers are cleared on completion. The factory toggle and shared page Refresh
control have 44-pixel mobile touch targets.

## Verification

- `npm run --workspace=apps/web typecheck` passed.
- API client and store unit suites: 35 passed. Coverage includes silent response
  bodies timing out, timer cleanup/retry, project switches and late replies.
- Focused headless Chromium workflows: 9 passed in 13.3 seconds across
  `factory-diagnostics.spec.ts`, `dashboard-freshness.spec.ts` and
  `review-holds.spec.ts`. The new workflow exercises initial failures, recovery,
  both endpoint refresh counts, failure after prior success, stale-evidence
  labeling and a subsequent successful empty decision. It confirms no start/stop
  mutation was made. Existing review-hold and freshness/reconnect checks pass.
- Desktop failure and mobile recovery captures were inspected. The confirmation
  pass verifies both mobile controls are at least 44 × 44 pixels and no main
  content horizontal overflow occurs at 390 pixels.
- Production web build and `git diff --check` passed. Owned test listeners on
  ports 38111/38775 were closed after completion.

This is a local shared-web repair using fixture APIs. It does not establish why
Michael's controller is stuck, recover a live review gate, deploy the dashboard,
or update the installed desktop/recovery ZIP. Live central authentication and
asset delivery remain unresolved; beta acceptance remains pending.
