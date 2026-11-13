# Worker links

A worker link is the network path a central uses to reach one registered
worker. All of it lives in `apps/server/src/workerLink.ts`. Every request the
central sends to a worker goes through `linkTo(node)`: health polls, quota
reads, native dispatch, fleet dispatch, worker chat, updates, login repair and
project import.

## What a link decides

A worker registers one address. From that address the link settles:

1. **The kind of path.** Derived from the address alone. Nobody declares it, so
   it cannot disagree with the address.
2. **Whether this central may use it.** Refusals name the address, the reason
   and the setting that would change the answer.
3. **The credential.** The worker's bearer token goes on every link except a
   worker on the same machine that never asked for one.

Requests never follow redirects and never leave the registered origin.

## Kinds

| Kind | Address | Accepted |
|---|---|---|
| `loopback` | `localhost`, `127.x`, `::1`. Also the near end of an SSH tunnel. | Always |
| `https` | Any `https://` origin: a reverse proxy, Tailscale Serve. | Always |
| `tailnet` | Plain HTTP to `100.64.0.0/10`, `fd7a:115c:a1e0::/48` or a `*.ts.net` name. WireGuard encrypts it. | When `GAH_WORKER_LINKS` lists `tailnet` |
| `lan` | Plain HTTP to any other address, on a network the operator trusts. | When `GAH_WORKER_LINKS` lists `lan` |

Set the list in the central server's environment, for example in
`/etc/gah/server.env`:

```
GAH_WORKER_LINKS=tailnet
```

Separate several kinds with commas. A central that lists only `tailnet` refuses
a worker that registers an ordinary LAN address.

`GAH_ALLOW_INSECURE_HTTP=1` predates this setting and still enables both
plain-HTTP kinds, so existing deployments keep working. It also lifts the TLS
requirement for requests arriving at that server, which `GAH_WORKER_LINKS` does
not. A central behind a TLS proxy can replace it with `GAH_WORKER_LINKS`.

The Nodes API (`GET /api/registry/nodes`) reports each node's `link`, or `null`
when the central refuses the address.

## What has not moved yet

- **The worker's side.** A worker accepts plain-HTTP requests from the central
  only with `GAH_ALLOW_INSECURE_HTTP=1`, which the WSL and macOS installers
  set. The same address classification could decide that, in
  `authMiddleware.ts` and `webSocketAuth.ts`.
- **`transport_mode`.** Workers still send it and the link refuses a mode the
  address contradicts. It no longer decides anything else. Retiring it touches
  the Rust CLI, the installers and the cloud registration RPC.
- **The worker's own address.** Each installer works out the address to
  register. `gah node register` takes it from the identity file or
  `--advertised-url`.

## Planned: Cloudflare Tunnel for a hosted central

A hosted central cannot join a customer's tailnet or LAN. The worker runs
`cloudflared` and the central calls the tunnel's hostname.

On the central this is the `https` kind plus a Cloudflare Access service token
(`CF-Access-Client-Id`, `CF-Access-Client-Secret`), so that only the hosted
central can reach the hostname. `headers()` in `workerLink.ts` adds the two
headers. No caller changes.

Still to design:

- Where the Access token is configured, and whether it is one per central or
  one per worker.
- Who creates the tunnel and its hostname at enrollment.
- The worker's side. `cloudflared` delivers requests from loopback with
  forwarding headers. The worker must treat them as remote and authenticated
  by the bearer token, without the plain-HTTP switch.
- Long uploads and streams. Native context transfer sends files of up to
  128 MB and worker chat streams for up to an hour. Both need checking against
  Cloudflare's request limits.
