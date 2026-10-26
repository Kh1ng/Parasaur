# GitLab integration

How Parasaur serves the GitLab panel: event ingestion starts the work, the
runs feed shows it inside the panel, and the embed view (#1593)
mounts both. The companion contract is
[docs/parasaur-contract.md](https://github.com/Kh1ng/parasaur-gitlab/blob/main/docs/parasaur-contract.md)
in the private [Kh1ng/parasaur-gitlab](https://github.com/Kh1ng/parasaur-gitlab)
repository; the epic is [#1591](https://github.com/Kh1ng/git-agent-harness/issues/1591).

## Event ingestion (#1643)

### Configuration

Set these environment variables on the central server:

- `GAH_GITLAB_HOOK_SECRET`: required shared secret. Use the file hook's `PARASAUR_HOOK_SECRET` value. Never put this secret in committed configuration.
- `GAH_GITLAB_BOT_USERNAME`: bot username. Default: `parasaur`.
- `GAH_GITLAB_PIPELINE_REPAIR`: set `1` to enable failed-pipeline repair. Default: off.
- `GAH_GITLAB_PANEL_URL`: required public dashboard base URL, including any path prefix. Example: `https://panel.example.com/parasaur/`.
- `GAH_GITLAB_EVENTS_RATE_LIMIT`: positive integer requests per minute per source IP. Default: `600`, also used for invalid values. Applies before secret verification and event classification, including unsupported events and invalid secrets.
- `GAH_GITLAB_EVENTS_STORE_PATH`: optional receipt-file path. Default: `$XDG_STATE_HOME/gah/gitlab-events.json`, or `~/.local/state/gah/gitlab-events.json`.

Configure a GitLab profile with `provider`, `repo`, `provider_api_base`, and `provider_project_id`.
Authenticate `glab` as the configured bot with the existing provider credentials.
The profile must match the event's project URL and repository path exactly.
Ambiguous profiles fail closed.

Use the existing profile policy:

```toml
[profiles.example.publishing]
trusted_issue_human_authors = ["maintainer"]
trusted_issue_bot_authors = ["parasaur"]
allow_issue_comments = true
```

GitLab has no implicit trusted owner. Empty or absent author lists grant no trust.
The hook uses `gah config show --json --full` to read these lists.
Older CLIs without `event_policy` cannot authorize event dispatch.
Comment policy and `delivery_mode = "handoff"` still apply.

### Request and response

Send `POST /api/integrations/gitlab/events` with `Content-Type: application/json`.
Send the shared secret in `X-Parasaur-Hook-Secret`.
The server compares fixed-size secret hashes in constant time.
The body is raw GitLab JSON, without an envelope.
No dashboard bearer token is required for this route.
Workers do not expose this integration.
Remote requests require HTTPS unless the existing insecure-HTTP override is enabled.
The standard JSON body limit is 100 KiB.

The companion file hook forwards stdin unchanged. It sends no event UUID header.
Its curl command has a ten-second timeout. It has no retry loop and does not fail on HTTP error status.
Use hook and server logs to diagnose rejected deliveries.

- `202`: the server saved a receipt and scheduled asynchronous authorization and dispatch.
- `204`: an unsupported event, an inactive trigger, or a duplicate receipt.
- `401`: the secret is absent, incorrect, or unconfigured.
- `429`: the per-source-IP request limit was exceeded. Default: 600 requests per minute.
- `503`: panel URL unavailable, receipt storage unavailable or full, or 64 asynchronous deliveries already pending.

Invalid JSON returns `400`. Oversized JSON returns `413`.
Authorization can reject a candidate after `202`; this starts no worker.
Provider failures produce a server log entry without provider output or credentials.

### Dispatch rules

The server loads the actor through `glab` and checks the existing human or bot author list.
It verifies the configured project ID through the provider API.
Issue assignment also requires a trusted issue author.
Issue bodies never select hook configuration, authorization, or dispatch modes.
Rust retains its issue intake, label, claims, routing, and publication checks.
Managed issues retain the existing dashboard dispatch fence.

- Issue assignment: an `open` event assigned to the configured bot, or an `update` that adds the bot. Dispatch mode: `improve`, target: `#<iid>`.
- MR review request: an `open` event with the bot reviewer, a reviewer addition, or a review re-request. Dispatch mode: `review`, MR: `<iid>`.
- Issue or MR note: a whole `@parasaur` mention in a non-system note. The note text is the worker instruction. Issue mode: `improve`. MR mode: `fix`.
- Failed pipeline: an enabled, failed branch pipeline with an open MR authored by the configured bot. The MR must match the project and pipeline SHA. Its branch must start with `gah/<repo_id>-`. Dispatch mode: `fix` on that MR.

Bot-authored mention notes are ignored. Commit and snippet notes are ignored.
Unrelated assignment or reviewer edits do not dispatch.
Pipeline events without an open bot-owned MR are ignored.
All starts use the same fleet coordinator as `POST /api/dispatch`, then the existing `gah dispatch` path.
Every dispatch records the session's `gitlab` context (see the runs feed below),
so panel queries scoped to the event's origin see the started runs.
A successful start posts `Parasaur queued a run` with a dashboard session link.
A comment failure never starts another run. This integration does not merge MRs.

### Redelivery and limits

If supplied, `X-Gitlab-Event-UUID` or body `event_uuid` identifies the event.
Otherwise, receipts use project, object kind, object ID, and action or pipeline status.
Issue and MR receipts also include revision and assignment changes. Note receipts include revision and note text.
The file stores only receipt hashes and expiry times, with private permissions and atomic replacement.
One central server owns the receipt file. Do not share it between server processes.
Receipts survive restart and expire after 24 hours. The store holds at most 10,000 unexpired receipts.
A full store refuses new events instead of removing live receipts.

The receipt is saved before `202`, so replay cannot repeat a dispatch after a comment failure or restart.
There is no durable work queue or automatic dispatch retry.
A crash after acknowledgement can leave a receipt without a run.
Before manual replay, check the run state and remove only the affected receipt hash from the file with the server stopped.

Fixtures under `apps/server/tests/fixtures/gitlab-19/` derive from GitLab 19.0 documentation examples.
They are sanitized examples, not live captures. Validate the deployment with real file-hook deliveries before enabling unattended work.

## Context-scoped runs feed (#1595)

`GET /api/integrations/gitlab/runs` returns the runs relevant to what the
panel is looking at: live dispatch sessions plus durable ledger runs for one
GitLab issue or merge request, newest first, with backend, model, test
status and the resulting MR link. It is a read behind the same auth as every
other `/api` route (bearer token, paired device cookie, or trusted local
request) — unlike the events endpoint, it never accepts the hook secret.
Only the central server exposes it; workers do not.

Query parameters:

- `project` (required): full project path, e.g. `group/project` — one or
  more non-empty `/`-separated segments of GitLab-allowed characters.
- `kind` (required): `issue` or `merge_request`.
- `iid` (required): positive issue or merge request iid.
- `gitlab` (optional): the GitLab origin, e.g. `https://gitlab.example.com`.
  It must be an `http(s)` origin configured for a GitLab profile; anything
  else is a `400`. When present, only sessions and profiles that can prove
  that origin answer; unprovable matches are excluded rather than assumed.
- `branch` (optional): further narrow runs to one branch.
- `limit` (optional): 1–100, default 20.

Responses:

- `200`: `{ context: { project, kind, iid, webUrl, profiles }, runs, events }`.
  `context.profiles` names the configured GitLab profiles matching the
  project (empty when none: the feed is then empty, not an error). `runs`
  and `events` are newest first and bounded by `limit`.
- `400`: empty or malformed context (`invalid_gitlab_context`) — a missing,
  malformed or unconfigured context never widens the feed.
- `502`: a `gah` CLI read failed.

Each run is `{ id, source: 'session' | 'ledger', status, mode, workId,
branch, backend, model, startedAt, endedAt, durationSeconds, testStatus,
mrUrl, title, profile, attempts }`. `session` runs are in-flight dispatches
(terminal ones are the ledger's job, so nothing is listed twice); `ledger`
runs are durable records with their per-attempt history in `attempts`.
`testStatus` is the ledger's `validation_result` (e.g. `passed`, `failed`,
`not_run`). `mrUrl` is the resulting MR link. A live-session run deep-links
its detail as `?page=work&profile=<profile>&session=<id>` (the same shape as
the events endpoint's queued-run comments); a ledger run links the
profile's work page.

### How runs are matched

The filter reuses what the server already exposes — the fleet session list,
the dispatch ledger (read through the same `gah report` ledger path as
`/api/report/roles`), and `gah events` — and matches a run to the context
by:

- the structured `gitlab` context recorded on the session by the GitLab
  event dispatcher (#1643): `options.gitlab = { project, kind, iid,
  webUrl }` on every dispatch it starts. A session carrying it must still
  pass the common provider/repo/branch guards before it counts;
- derivation for older sessions and ledger entries: GitLab provider +
  project path, `source_issue_number` / `#<iid>` work ids for issues,
  `mr:<iid>` work ids and MR URLs for merge requests, and the authoritative
  resource URL embedded in note-instruction targets. A derived match
  requires the run's recorded repo/host and its issue/MR reference —
  parsed from the authoritative resource URL, not from arbitrary body
  links — to equal the requested context; a link in a body to another
  project is never context.

`POST /api/dispatch` and the `session.start` WebSocket message accept the
same optional `gitlab` context, validated by the same runtime validator on
both paths, so any start path can record it and the feed stays
provider-scoped without string parsing.

## Embed view for the GitLab panel (#1593)

The panel (`panel/parasaur-gitlab.js` in the companion repo) adds a dino
button to GitLab that opens a side panel iframing Parasaur's chrome-less
embed view with the current page's context.

### Route

`GET <parasaurUrl>embed?gitlab=<origin>&url=<page url>&project=<full
path>&kind=<issue|merge_request|file|tree|pipeline|job>&iid=<number>&ref=<ref>&path=<path>`

- `GET /embed` renders the same SPA without the app shell: the agent chat
  with its model and skills pickers, context actions ("Work on this issue",
  "Review this MR", "Fix review comments", "Explain diff"), and the runs
  for this issue or merge request. The context is read from the query
  string and refreshed over the postMessage handshake when the GitLab page
  changes.
- `GET /?gitlab=…` (same query) is the standalone URL the panel's "open in
  Parasaur" button opens: the same view inside the normal app shell.

### Framing policy

- `/embed` is frameable **only** by the origins configured in
  `GAH_GITLAB_ORIGIN`: the response carries
  `Content-Security-Policy: frame-ancestors <origins>`. The value is a
  space- or comma-separated list of exact http(s) origins
  (`https://gitlab.example.com`). Entries containing a wildcard (`*`), a
  missing scheme, or a path are rejected. With no valid origin configured
  the route fails closed: `frame-ancestors 'none'`, so no site can frame
  it.
- `/embed` never carries `X-Frame-Options`.
- Every other served HTML document — including `/` and `/index.html` —
  carries `X-Frame-Options: DENY`; the rest of the app stays
  non-frameable.
- The framing headers are emitted by the Parasaur server, so the server
  install must serve the web app itself (`GAH_WEB_ROOT` set, the default
  service setup). If nginx serves the static files directly, it must add
  the same headers itself.

### Authentication

The embed view uses the same `/api` and WebSocket authentication as the
dashboard. Owner tokens stay in tab-scoped `sessionStorage`; they are never
broadcast through `localStorage` or postMessage. Device pairing uses the
existing HttpOnly device cookie. The supported install proxies Parasaur
on the GitLab origin itself (`https://gitlab.example/parasaur/`), so the
iframe is same-origin and first-party. GitLab scripts on this shared origin
are trusted and can access the iframe's sessionStorage.

An unauthenticated panel opens a prefixed standalone sign-in URL. For owner
sign-in, the server issues a random handoff code and a separate verifier.
The iframe holds the verifier only in memory; only the code goes into the
sign-in link's fragment, which the sign-in tab removes from its URL. After
entering the owner token, choose **Connect GitLab panel**. The sign-in tab
POSTs the code with its authenticated session. The iframe polls the server
with its code and verifier, receives the token in an HTTP response once,
and stores it in its own sessionStorage. The server stores only hashes,
origin, approval state, and a 60-second expiry in memory. A second tab with
just the code cannot claim it. Expired or consumed codes cannot be reused;
choose **Retry** in the panel to prepare a new link. Cookie-based pairing
sign-ins are also detected by polling `/api/pairing/session`.

No owner token travels in a URL or through the embedding page. Per-user
GitLab identity (SSO) is a separate epic issue (#1596).

The device-approval flow is prefix-aware: the pairing sign-in and approval
URLs are built from the public path prefix, and the browser-bound approval
credential cookie is encrypted with an in-memory AES-GCM key and scoped
to `<prefix>/api/pairing/access` so the prefixed status and claim requests
still carry it. Pending requests and their encryption key expire on server
restart; already paired device sessions are unaffected.

### postMessage API

All messages are validated against `event.origin` matching
`GAH_GITLAB_ORIGIN`; everything else is ignored. Outbound messages are
addressed to the configured origins only, never `*`.

| Direction | Message |
|---|---|
| panel → Parasaur | `{type:'parasaur:context', context}` when the GitLab page changes or the panel reopens |
| Parasaur → panel | `{type:'parasaur:ready'}` once loaded (the panel replies with the context) |
| Parasaur → panel | `{type:'parasaur:close'}` |
| Parasaur → panel | `{type:'parasaur:navigate', path:'/group/project/-/merge_requests/42'}` (same-origin absolute paths only, bounded and validated) |

### Path prefix (`X-Forwarded-Prefix`)

The supported install proxies Parasaur at `https://gitlab.example/parasaur/`
(nginx `location /parasaur/`). The Parasaur server reads
`X-Forwarded-Prefix` (trusted from the loopback reverse proxy, like the
other `X-Forwarded-*` headers) and injects into every served document:

- `<base href="/parasaur/">`, so the build's relative asset URLs resolve
  under the prefix;
- `window.__GAH_BOOTSTRAP__ = { basePath, gitLabOrigins }`, which the SPA
  re-validates and uses to build API and WebSocket URLs
  (`${origin}${basePath}/api/…`, `${…}${basePath}/ws`), to build
  prefix-aware sign-in and approval URLs, and to filter postMessage
  origins.

The web build uses relative asset URLs (vite `base: './'`), so one build
serves both the root deployment and a prefixed proxy install. The offline
service worker is a root-deployment feature and is not registered under a
prefix.

### nginx expectations

```nginx
location /parasaur/ {
    # Only signed-in GitLab users reach Parasaur; the GitLab session cookie
    # is stripped (companion repo's gitlab.rb snippet).
    proxy_pass http://127.0.0.1:3773/;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-Proto https;   # TLS terminates at nginx
    proxy_set_header X-Forwarded-Prefix /parasaur;
    # WebSocket upgrade (the /ws endpoint) passes through the same location.
}
```

- `GAH_WS_ALLOWED_ORIGINS=https://gitlab.example` must be set on the
  server: the WebSocket origin check requires public hostnames to be
  allow-listed (`docs/WEBSOCKET_AUTH_532.md`).
- Set on the server: `GAH_GITLAB_ORIGIN=https://gitlab.example`
  (framing + postMessage), and `GAH_WEB_ROOT` to the built web app.
- `GAH_GITLAB_PANEL_URL` (from #1592) should name the same public base
  URL, `https://gitlab.example/parasaur/`, for run links.

## Panel identity hook for enterprise sign-in

Public core exposes `GitLabIdentityProvider` in `apps/server/src/integrations/gitlab/identity.ts`.
This extension point does not implement GitLab or Supabase sign-in.
The default `anonymousGitLabIdentityProvider` always returns `null`.
Without a registered provider, all identity and assertion headers are inert.
HTTP responses and WebSocket upgrades retain their existing authentication behavior.
Explicit owner or paired-device credentials take precedence, even when an assertion is present.
Invalid explicit credentials still fail through the existing credential checks.
Only requests without explicit credentials consult a registered provider, before local or trusted-LAN fallback.
An assertion on that path must validate or the request is denied; without an assertion, existing authentication applies.

The companion contract currently defines only the nginx gate and cookie stripping.
The gate admits signed-in GitLab users to `/parasaur/` and removes the entire `Cookie` header upstream.
It does not currently issue a signed identity assertion.
`X-Parasaur-Identity-Assertion` is the public-core extension contract for the future enterprise module.
Neither the gate, `GAH_GITLAB_ORIGIN`, nor a forwarded username grants identity by itself.

Register the enterprise provider through `createServer({ deviceAccess, gitLabIdentityProvider })` before the server starts.
Registration is local to that central server, never a request parameter or environment-selected module.
For a custom startup, pass `app.locals.gitLabIdentity` as the fifth argument to `createAuthorizedWebSocketServer`.
Use the same `DeviceAccess` instance for both servers.
The standard `bin.ts` already passes this instance.
Workers do not accept this hook.

The provider implements this interface:

```ts
interface GitLabIdentityProvider {
  authenticate(request: GitLabIdentityRequest):
    GitLabPrincipal | null | Promise<GitLabPrincipal | null>;
}

interface GitLabPrincipal {
  id: string;
  displayName: string;
  role: 'controller';
  expiresAt: string;
}
```

`GitLabIdentityRequest` contains only these fields:

| Field | Meaning |
| --- | --- |
| `assertion` | Opaque `X-Parasaur-Identity-Assertion` value, 1–8192 ASCII letters, digits, underscores, dots, or hyphens. Duplicate or malformed values fail closed. |
| `method`, `path` | HTTP method and upstream request URL, including its query. nginx removes `/parasaur` before proxying. WebSocket upgrades use the same contract. |
| `host`, `origin` | Raw `Host` and `Origin` headers, if present. These values are request evidence, not verified identity. |
| `forwardedPrefix` | Raw `X-Forwarded-Prefix`, normally `/parasaur`. This value grants no trust. |
| `remoteAddress`, `secure` | Immediate socket address and transport status. HTTP uses Express's existing loopback proxy policy. WebSockets use their existing transport policy. |

The hook never receives cookies, bearer credentials, arbitrary headers, or a mutable Express request.
The enterprise provider must verify the assertion signature, issuer, audience, expiry, request binding, and replay policy.
It must resolve the verified GitLab or Supabase subject to a known Parasaur principal.
It must return `null` for an unknown, unauthorized, or disabled user.
Public core does not parse assertions or trust identity headers without a registered provider.
Provider errors on the assertion path produce a generic `401`, without provider details or fallback to local or trusted-LAN access.

For this hook, `id` must name an existing active paired controller in `DeviceAccess`.
It is not a raw GitLab username or an automatically provisioned Supabase ID.
The provider owns that mapping and returns a nonempty display name of at most 80 characters, without control characters.
`expiresAt` is an absolute ISO-8601 timestamp.
Public core rejects unknown IDs, revoked controllers, expired claims, malformed results, and unsupported roles.
It caps the session expiry at the local controller expiry.

Successful authentication sets `res.locals.authPrincipal` to `{ kind: 'device', id, displayName, role, expiresAt }`.
`GET /api/pairing/session` exposes this principal through the existing session response.
`webSocketGitLabPrincipal(ws)` exposes the verified attribution to enterprise chat and dispatch consumers.
Existing mutation audits retain `device:<id>` as the actor.
The session retains controller permissions to read dashboard data, use chat, and dispatch work.
Owner-only settings, credential management, and paid-route approval mutations remain forbidden.
Existing owner-delegated pairing approval grants still apply to the mapped controller.
This hook adds no new paid-route approval roles or per-project permissions.

The hook mints no cookie or bearer token and creates no device record.
Each API request and WebSocket upgrade authenticated through the hook requires a fresh provider check.
WebSocket sessions terminate at the earlier claim or controller expiry, or upon local controller revocation.
Buffered messages also check that expiry and revocation state.
Enterprise revocation must revoke the mapped local controller or wait for its assertion expiry to end an existing socket.

For deployment, strip client-supplied assertion headers before the trusted enterprise gate creates its own assertion.
Keep the GitLab session cookie inside GitLab.
Preserve the public `Host` and HTTPS scheme so existing origin and transport checks succeed.
The current companion snippet uses the upstream host and needs that deployment adjustment for enterprise sign-in.
Forward the assertion on API calls and WebSocket upgrades under the panel prefix.
The `/embed` document router, framing policy, sign-in handoff, and runs-feed routes need no duplicate implementation.

Enterprise must still choose the issuer, signing keys, audience, claim lifetime, replay controls, and principal enrollment policy.
It must also implement GitLab/Supabase sign-in and the trusted gate changes before this hook can authenticate panel users.
