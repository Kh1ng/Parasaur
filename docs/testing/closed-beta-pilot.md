# Hosted closed beta pilot (#1606)

This gate requires the hosted project, two real accounts provisioned by the owner,
and a tester-controlled remote node. Local regression tests and mocked sessions
cannot establish acceptance. Keep #1606 open until every acceptance row below has
observed evidence and findings have issue links.

## Prepare

1. Record the dashboard, central, and worker revisions, browser/OS versions, and
   UTC start time in a private session artifact. Use aliases for accounts,
   workspace, node, and job in public reports; retain the mapping privately.
2. Have the owner complete [closed beta provisioning](../CLOSED_BETA.md), including
   hosted sign-up verification and current entitlements for both accounts.
   The tester must not already belong to the workspace.
3. Use separate browser profiles for owner and tester. Confirm that the tester
   cannot access the workspace before accepting an invitation.
4. Prepare an approved disposable repository and one bounded test ticket. Its
   acceptance should require a harmless file change and a verification command.
   Disable automatic publishing for this pilot; do not push or create PRs/MRs.
5. Prepare the remote worker's repository and backend credentials privately.
   Follow [cloud node registration](../cloud-node-registration.md) for hosted
   membership, device enrollment, registration, and heartbeat prerequisites.
   Missing enrollment or hosted access is BLOCKED; do not substitute a service-role
   key on the worker.

## Run in order

1. As owner, open **Settings → Cloud hosting**, select the workspace, enter the
   tester under **Teammate account**, and select **Invite to workspace**.
2. As tester, accept using **Join workspace** in the **Workspace invitation**
   dialog. Confirm workspace access and membership in both sessions. Capture
   redacted before/after evidence. Direct SQL membership creation or RPC-only
   acceptance does not verify the in-app flow.
3. Connect the remote node using the supported setup for its platform. Run it in
   hosted mode with the tester's membership and enrolled device credential. Confirm
   the intended node and profile appear in the integrated dashboard. Observe a
   heartbeat renewal and healthy status; registration alone is insufficient.
4. Check readiness for the selected backend and repository on that node. Dispatch
   the prepared ticket to that remote node through the normal dispatch flow.
   Record the job/attempt and node correlation in the ledger/session artifacts.
   A chat request alone does not establish a dispatched-job result.
5. Watch the integrated dashboard during execution. Confirm that events for this
   job appear, the executing node matches the selected remote node, and node health
   remains visible. Wait for the job's terminal result; inspect its file change and
   verification output against the prepared ticket's acceptance criteria.
6. Reload the dashboard and confirm the same job result, events, and node health
   remain accessible. Record UTC completion time and redacted evidence references.

Stop at a failed prerequisite rather than bypassing the flow. Record FAIL for an
observed mismatch and BLOCKED for an unavailable prerequisite. Preserve missing
model, usage, cost, quota, or outcome telemetry as unknown. Record backend instance,
requested model, actual model, and usage class separately when available.

## Findings and closure log

For each finding, prepare an issue with revision, failing step, expected/actual
behavior, reproducible steps, and redacted evidence. The authorized human files it
and adds its link to the log. Never publish passwords, tokens, invite codes,
project URLs, emails, user UUIDs, or private repository contents.

Copy this template into the session artifact, then post a redacted short log on
#1606 after the human run. Blank fields are unverified, not PASS.

```text
Pilot #1606: <UTC start/end>; tester: <alias>; revisions: <dashboard/central/worker>
Hosted owner prerequisites: <PASS/FAIL/BLOCKED + evidence>
In-app invite accepted by second account: <PASS/FAIL/BLOCKED + evidence>
Remote node connected, heartbeat renewed, healthy: <PASS/FAIL/BLOCKED + evidence>
One dispatched job completed and acceptance verified: <PASS/FAIL/BLOCKED + evidence>
Integrated dashboard events and node health, including reload: <PASS/FAIL/BLOCKED + evidence>
Ledger/session evidence: <private artifact reference; public redacted reference>
Findings: <issue links, or explicitly none observed>
Gate: <PASS only when all acceptance rows pass; otherwise FAIL/BLOCKED>
```

Local checks supporting this procedure:

```sh
bash scripts/test-supabase-migrations.sh
node --test scripts/supabase-hygiene.test.mjs
```

These checks cover provisioning regressions and repository hygiene. They do not
replace the hosted pilot or authorize closing its human-required gate.
