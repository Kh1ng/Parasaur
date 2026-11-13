# Native provider claim lifecycle

Native fix/review jobs now use the existing GitHub assignee/comment claim
protocol. The default TTL, verification window, priority ordering, expiry checks,
takeover detection and local-work preservation rules are unchanged. Handoff
delivery refuses provider-visible claims before assignment.

Each assignment, removal, claim comment and renewal write requires fresh central
authority. Claim operations have their own bounded one-entry journal and monotonic
sequence, separate from the backend/publication sequence. A background renewal
can therefore run during a long backend attempt without completing or replacing
that attempt's fence. Both channels require the original consumed start, held
central claim, registered worker, current policy, route and account approvals.
Lost replies cannot replay a sequence. A pending or uncertain claim write blocks
later execution/publication and successful final completion.

The job serializes claim writes against backend admission/completion and provider
publication. Provider commands have an owned process group, a deadline no later
than the live claim's remaining lease (capped at 30 seconds), and a 1 MiB capture
limit per stream. An expired deadline cannot launch a command. Child cleanup is
confirmed before completing its write fence, while the backend may still run in
its separate process group. The service credential remains process-private.

Real worker acceptance exposed contention with concurrent status observations:
an observation briefly holds the same journal lock, so the job process could fail
to consume its start, or retain an unconfirmed fence after a confirmed provider
write. Every in-job journal open (start offer, start consumption, authorization
and completion) now waits up to two seconds only for a busy lock. It never
retries HTTP authorization, provider writes or uncertain state transitions. A
focused test holds and releases the observation lock to verify that behavior.

`scripts/tests/native_controller_http.mjs` adds provider-claim modes with a
23-second backend fixture. Its declared fixture policy uses a one-minute TTL and
zero-second verification window so renewal occurs during that attempt; production
defaults remain unchanged. The fixture checks normal renewal, central factory
pause, and takeover by a different provider identity. It records actual job exit,
validation result and usage separately: passing validation remains a measured
fact even when lost operation authority makes the overall job fail.

## Real controller acceptance

The acceptance uses the matched private API build pinned to public
`f8725235` (private `a942d05a`, isolated checkout
`/home/khing/.cache/parasaur-enterprise-current.a06cvwh2`) and a debug Linux
CLI built from this working tree, SHA-256
`e2ec5a8505b784c01fbe0cd57aa0ec82cce2dc2546e1af949a072dbdf16eb935`. The
first rerun of the normal provider-claim mode failed before the wider lock wait:
the job could not consume its start because an observation held the journal
lock, and the broker reported the worker state unconfirmed. After the change,
all four modes pass with one backend attempt, one canonical import, an
acknowledged capacity release, no relaunch or re-import across central reload
and no production provider, controller or installed worker change.

| Mode | Job outcome | Validation | Exit | Renewal during backend | Stopped work not redispatched |
| --- | --- | --- | --- | --- | --- |
| normal | passed | passed | 0 | n/a | n/a |
| provider-claims | passed | passed | 0 | yes | n/a |
| provider-claims-paused | failed | passed | 1 | refused | yes |
| provider-claims-lost | failed | passed | 1 | refused | yes |

Fixture roots are `parasaur-controller-http.si7cbf`, `.pf1jzy`, `.PNMWoC`
and `.HexQ5c` under `/home/khing/.cache`, each with a `report.json`. The pause
mode ends with the controller's own "Factory automation is disabled" refusal
rather than a completed cycle. The lost mode leaves the other identity's
assignment and second claim comment in place and makes no renewal write.

Rust checks pass under WSL: 58 native handoff, 579 dispatch, 76 provider, three
CLI handoff and 12 capability tests, formatting and all-target Clippy with
warnings denied. Log:
`/home/khing/.cache/parasaur-project-report.qy1ih6c9/native-dispatch-checks.log`.
An optimized matched release build and installation remain pending.
