# Hosted migrations and node-health owner handoff

Issue #1605 requires the project owner's linked Supabase CLI and access to the
private enterprise repository. The public schema alone cannot verify the deployed
`node-health` Edge Function. Complete this procedure on the owner's machine.
Keep credentials, linked CLI state, project identifiers, and raw private responses
out of commits and public issue comments.

## Verify and apply migrations

From the intended checkout, first run the local regression commands:

```sh
bash scripts/test-supabase-migrations.sh
node --test scripts/supabase-hygiene.test.mjs
```

Confirm privately that the CLI is linked to the intended hosted Parasaur project.
Then inspect the remote history and proposed changes:

```sh
supabase migration list --linked
supabase db push --linked --dry-run
```

Review every pending migration before applying. Resolve divergent histories before
continuing; do not mark migrations applied without executing them. With owner
authorization for the reviewed deployment, run:

```sh
supabase db push --linked
supabase migration list --linked
```

Compare every local version to its remote entry. The ticket's minimum boundary is
`20261009000000_handoff_race.sql`; this checkout also includes the two
`20261024` authorization fixes. Acceptance requires every committed migration to
be applied, including those fixes. A successful push alone is insufficient evidence.

## Deploy and verify node-health

Use the private enterprise repository's deployment instructions and current
`node-health` request contract to deploy the updated function to the same project.
Verify its authentication configuration as part of that deployment. Do not infer
the Edge Function's headers or JSON body from the SQL RPC signature.

Using a provisioned test node and its valid device credential:

1. Record the node's current `worker_heartbeat_at` and the server time privately.
2. Wait at least 10 seconds after its previous accepted heartbeat and pause other
   heartbeat senders for that test node so they cannot mask the result.
3. Send an authenticated POST to the deployed `node-health` endpoint using the
   private function's request contract. Require a successful HTTP response and
   verify the expected workspace and node identity.
4. Verify that `worker_heartbeat_at` advanced and `lease_expires_at` equals that
   server timestamp plus 90 seconds. Repeat after the 10-second rate limit and
   confirm that the lease advances again. A throttled response retaining the
   previous lease does not prove renewal.
5. Repeat the POST with a deliberately invalid device credential while retaining
   any otherwise-required API authentication. Require an authentication rejection
   and verify that the node's heartbeat and lease did not advance. A gateway
   failure caused only by a missing API key does not prove device-token rejection.
6. Restore the test node's normal heartbeat sender.

The SQL behavior is defined in
`supabase/migrations/20261004000005_node_health.sql`: accepted heartbeats use
server time, the minimum interval is 10 seconds, and leases last 90 seconds.
An RPC-only test does not verify the deployed Edge Function.

## Record acceptance evidence

Record results in #1605 and private enterprise issue #1, with private evidence
referenced through an access-controlled location. Include the operator, UTC time,
public and enterprise commit IDs, migration comparison result, deployed function
revision, HTTP statuses, and redacted timestamp comparisons for both renewals and
the invalid-token rejection. Never include tokens or raw authenticated requests.
Close enterprise issue #1 only after every live check passes.

Use this result template with observed values:

```text
Operator / UTC time:
Public / enterprise revisions:
All local migration versions present remotely:
Deployed node-health revision:
First POST status / heartbeat / lease expiry / 90-second delta:
Second POST status / heartbeat / lease expiry / renewal confirmed:
Invalid-device-token status / heartbeat and lease unchanged:
Private evidence reference:
Enterprise issue #1 closure reference:
```

Missing hosted evidence leaves the task pending owner verification. Local tests
and this handoff do not satisfy the live acceptance criteria.
