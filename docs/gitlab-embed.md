# GitLab panel embed

Set `GAH_GITLAB_ORIGINS=https://gitlab.example` on the Parasaur server. Multiple
origins can be separated by spaces or commas; use exact HTTP(S) origins, without
paths or trailing slashes. The allowlist controls both the `/embed` framing CSP
and the browser message protocol. An empty allowlist denies all framing. Other
routes keep `frame-ancestors 'none'` and `X-Frame-Options: DENY`.

The panel URL is `/embed?gitlab=&url=&project=&kind=&iid=&ref=&path=`. `gitlab` is
the GitLab origin, `project` is the namespace/project path, and `url` must belong
to that project on that origin. Use `kind=mr` (or `merge_request`) or `kind=issue`
for the corresponding actions. The project must already be configured in
Parasaur with a matching GitLab repository URL. Starting a context chat creates
a separate conversation and worktree; action buttons prepare a prompt for the
user to send. The existing model and skills controls apply to that conversation.

The iframe sends `{type: 'parasaur:ready'}` to configured origins after installing
its message listener. The parent can send
`{type: 'parasaur:context', context: {gitlab, url, project, kind, iid, ref, path}}`.
Both the exact origin and the parent window are checked. The iframe sends
`{type: 'parasaur:close'}` on Close and
`{type: 'parasaur:navigate', path: '/namespace/project/...'}` on navigation.
Navigation messages contain only same-origin GitLab paths. Page context is data
included with each chat message, not a source of authorization. Existing server
authentication and permissions still apply.

“Open in Parasaur” opens the same context with `standalone=1` and the normal app
chrome. The configured origin allowlist still applies to its page context.

For an installation at `https://gitlab.example/parasaur/`, have the trusted reverse
proxy strip `/parasaur` before forwarding HTTP and WebSocket requests and set
`X-Forwarded-Prefix: /parasaur` on HTML requests. Serve the built dashboard through
`GAH_WEB_ROOT`. The HTML entry supplies the asset base and runtime prefix for REST
and WebSocket clients. Proxy `/parasaur/ws` with WebSocket upgrade support. Strip
client-supplied forwarded headers before setting the prefix at the proxy.
