# Panel identity adapter

Public core exports `PanelIdentityProvider` from `gah-server`. Enterprise hosts
supply the same provider to both HTTP and WebSocket entry points:

```ts
const app = createServer({ panelIdentityProvider: provider });
const server = http.createServer(app);
const wss = createAuthorizedWebSocketServer(
  server, 'central', deviceAccess, Date.now, provider,
);
createWebSocketHandler(wss);
```

An adapter's asynchronous `authenticate(IncomingMessage)` verifies a GitLab
session (for example through the enterprise session service), returning
`{ issuer, subject, canApprovePaidRoutes }`. Use a stable GitLab instance URL
and immutable user ID, not a username. Only return the approval permission for
users authorized by server-side policy. Never derive it from a request body.

A provider must verify credentials itself. Core does not interpret GitLab or
forwarded identity headers. An adapter accepting proxy assertions must
independently authenticate the proxy channel and ensure the proxy strips
client-supplied assertions. Loopback proximity and `trust proxy` alone do not
prove an assertion. Existing TLS and WebSocket origin checks still apply. HTTP requests carrying
cookies or an Origin must prove the same origin; adapters must also enforce
any additional session-specific CSRF requirements.

With an adapter configured, missing identities return 401 and verifier errors
fail closed; local access and coordinator credentials do not substitute for a
panel identity. Without an adapter, owner/device behavior is unchanged and
GitLab headers convey no identity. Mount both hooks consistently; the stock
standalone binary does not enable a GitLab adapter or implement OAuth.

Panel users may perform ordinary work, but do not acquire owner access to
administration, device-pairing approvals, or other owner-only operations.
Paid-route grant/revoke operations additionally require
`canApprovePaidRoutes: true`; their exact scopes and replay guards still apply.
WebSocket identities are verified again before each message, so a
revoked session cannot continue chat or dispatch work on an existing socket.

HTTP writes and WebSocket messages record attribution in
`config/mutations/panel-actions.jsonl` (or `GAH_MUTATION_STORE_PATH`). Records
contain the issuer/subject actor, timestamp, action, request ID and payload
hash, never credentials or chat text. HTTP responses expose the generated ID
in `X-GAH-Identity-Request-Id`; WebSocket records use the protocol request ID.
These records describe requests before execution, not proof of success.
Approval outcomes and actor-specific replay receipts remain in the existing
mutation audit. Attribution storage failure prevents new work. Protect and
retain these artifacts alongside other session artifacts.
