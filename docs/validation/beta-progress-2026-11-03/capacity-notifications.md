# Capacity waits and failure notifications

The live feed audit at 2026-11-06T06:26:46Z found 314 recognizable pre-launch
capacity refusals among 573 retained notifications. Of 192 unread alerts,
189 were these refusals. The controller correctly recorded an admission wait,
but the server converted every unsuccessful terminal dispatch into Work failed.
The bell therefore accumulated failures for jobs whose backend never started.

Public `7082c203` adds the history-only `dispatch_deferred` kind. Only explicit
controller receipts beginning with a typed deferred-capacity prefix followed
by node admission deferred or no eligible backend qualify. Fallback refusals
after real backend attempts, unfamiliar failures, human gates and gateway
errors retain their alerts. Known old controller notifications are corrected
on load, preserving their ids, original messages, timestamps, existing read
times and delivery receipts. They leave notification/unread lists but remain
in activity history under the normal retention policy. No existing real
notification is marked read by this correction.

`69a7da30` negotiates this new kind with `supportsCapacityWaitActivity`.
Older connected clients do not receive an event kind their UI cannot render;
their real alerts and unread counts continue normally. Updated clients see
quiet capacity receipts in All activity. The icon lookup also has a fallback
for future kinds. `0ce6e7fb` bounds the protocol regression's replay wait.

## Verification

- The public notification-only backport passes 33 Linux tests across feed,
  durable migration, preference, controller-history and real WebSocket files.
  The real-socket test covers both initial replay and subsequent live events
  for capable and legacy clients. A native Windows focused run hit two existing
  POSIX file-mode assertions; the Linux run verifies those permissions.
- Both server and web typechecks pass. All 13 activity browser cases pass,
  including the history-only receipt and real-notification boundary. Initial
  selectors targeted the wrong role/container; the test now uses the actual
  Activity sidebar and its tabs without weakening the content checks.
- Private assembly `dad5f82`, pinned to `0ce6e7fb`, passes 1,109 server tests
  (38 explicit skips), 16 mock cases, production builds/typecheck and private
  artifact/input/transport checks. Its log is
  `/home/khing/.cache/parasaur-enterprise-current.ztp5xbi_/build.log`.

## Live deployment

Shared web `0ce6e7fb` is deployed with all 30 served assets byte-verified.
The API runs public base `6b101b98` plus the notification/protocol changes
through `0ce6e7fb`. This deliberately excludes the staged persistent-auth
change while the actual GitLab proxy still awaits inspection. The source
patch, before/after hashes and build manifest are in ignored
`artifacts/parasaur-notifications-0ce6e7fb/`. Three JavaScript files and the
contract declaration were replaced. Their compiled base hashes matched
production exactly; the managed runtime's dependency lock matched the build.
The older checkout root lock differs, but is not the managed runtime lock and
was not replaced. No npm install or CLI/controller replacement ran on the VM.

The initial 35-second startup check rolled code back after the listener had
not appeared. The new process had reached the existing provider-status refresh;
no module, syntax, allocation or port-conflict error was observed. The previous
code recovered successfully. The activity migration had already persisted and
was retained, rather than restoring old notification data. That failed attempt
and the startup trace remain recorded. A fresh preflight then checked 100 chat
sessions, no active chats and an explicit empty job roster. A missing welcome
message is treated as unknown, never an empty roster.

The second rollout, with a process-identity guard and a 120-second cold-start
allowance, passed at 2026-11-06T06:46:06Z. API PID is 522721. All 260 retained
real notifications in the verification window kept their content, read state
and delivery history. The chat index, node identity, config, owner credential
and legacy handoff bytes were preserved. Controller PID 437234 was unchanged.
Code and activity backups are retained under the existing central repair
evidence directory; rollback does not restore over newer activity data.

A read-only live Tailnet TLS WebSocket check passed at 06:47:09Z: a capable
client received 198 quiet capacity receipts in its 200-event replay; a legacy
client received only the two supported events. No events or paid agent jobs
were injected. Reports are `capacity-notification-{web,api}-deployment.json`
and `capacity-notification-protocol.json` in local ignored
`artifacts/central-live-20261105/` and the corresponding VM evidence directory.

The physical capacity wait remains. The pre-listen provider-status refresh
also warrants startup hardening; this deployment does not claim that cold-start
blocking is resolved. Real pipeline progression, persistent GitLab sign-in,
provider billing reconciliation and Mac memory acceptance remain open.
