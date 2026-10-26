# Cloud node registration

Set `[defaults] orchestration_mode = "hosted"` or `"self_hosted"` to enable
cloud registration. Local mode makes no cloud requests.

The CLI reads these environment variables:

| Variable | Value |
| --- | --- |
| `GAH_SUPABASE_URL` | Your Supabase base URL |
| `GAH_SUPABASE_ANON_KEY` | Your project's public API key |
| `GAH_SUPABASE_WORKSPACE_ID` | Workspace UUID assigned by the operator |
| `GAH_SUPABASE_ACCESS_TOKEN` | A signed-in workspace member's Supabase user access token |
| `GAH_SUPABASE_DEVICE_TOKEN` | Device credential enrolled for this workspace and node |

The public key alone cannot register a node. Registration requires an active
workspace membership and, on hosted deployments, an active entitlement. The
operator provisions accounts and device credentials out of band. Use the device
credential for the same node ID as `config/coordinator-identity.json`, or the file
selected by `GAH_COORDINATOR_IDENTITY_PATH`. For a new identity, set
`GAH_NODE_ADVERTISED_URL` to the HTTP origin for this node. Never distribute a
service-role key to workers. The CLI does not read it.

`gah node register` first completes central registration, then mirrors the
identity to the cloud. `gah loop`, including `--once`, starts a background thread
that syncs immediately, even without `registry_central_url`. It renews every 30
seconds, independently of controller work, pacing, and error backoff. Local mode
starts no thread and makes no cloud requests.

The sink calls `POST {GAH_SUPABASE_URL}/rest/v1/rpc/register_cloud_node` with the
user token in `Authorization: Bearer` and the public key in `apikey`. This RPC
upserts identity and profiles. It excludes the local coordinator token and
`secret_ref`. Identity, profiles, endpoint, or credential changes cause another
upsert. Failed upserts retry on the next timer interval. Config reloads update
the existing thread without delaying its next heartbeat. Switching to local mode
stops the thread.

After registration, each timer interval calls
`POST {GAH_SUPABASE_URL}/rest/v1/rpc/cloud_node_heartbeat` with the public key in
both headers and `{"p_device_token":"<device credential>"}` in the body. The
server resolves the device identity and renews its 90-second lease. Registration
alone does not mark a worker online. Missing device credentials allow the upsert
but produce an advisory warning instead of a heartbeat.

Each HTTP request has a three-second timeout. Cloud failures never turn a
successful local registration or controller pass into an error. The sink counts
failed attempts and warns once whenever the failure kind changes, including the
HTTP status. It logs recovery once and warns again on a later outage. Logs omit
URLs, credentials, response bodies, and curl error details. Cloud HTTPS requests always
verify TLS, regardless of coordinator TLS overrides.

While waiting, the thread checks the controller shutdown flag at most 250 milliseconds apart.
When the controller exits, it cancels the timer wait and joins the thread. An
in-flight sync can take up to two three-second HTTP timeouts to finish.

PR #1608 contains the schema migrations that define these RPCs. Before you enable
cloud mode, deploy those migrations. User access-token refresh and device
enrollment are outside this registration sink.
