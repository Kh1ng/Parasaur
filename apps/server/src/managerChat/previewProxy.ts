/**
 * Session preview proxy (WP3 environments): one dedicated listen port per
 * chat session, proxying HTTP + WebSocket traffic to the dev server the
 * agent started inside the session's worktree (127.0.0.1:<devPort>).
 *
 * Why a dedicated port instead of a path prefix (/preview/<session>/...):
 * dev servers emit absolute asset URLs (/@vite/client, /src/main.tsx), so
 * prefix-stripping proxies 404 on the first reload. A real port per session
 * needs no rewriting at all.
 *
 * The proxied request's Host header is rewritten to `localhost:<devPort>`:
 * Vite (5.4+) and friends reject foreign Hosts as DNS-rebind protection,
 * and the app inside the worktree only ever expects localhost.
 *
 * Listen ports come from a small dedicated range (default 41000-41099) so
 * the operator can firewall exactly the preview surface. The advertised URL
 * uses the node's Tailscale IPv4 when present (WireGuard-encrypted in
 * transit), else the loopback for local dev.
 *
 * Every listener requires its own random access secret. The advertised URL
 * carries it once as a query parameter; the proxy exchanges it for an
 * HttpOnly cookie scoped by name to that listener and redirects to the
 * clean URL. Requests and upgrades without a valid secret are refused
 * before any upstream connection is made, so only clients that received
 * the URL from the authenticated control plane can reach the dev server.
 */

import http from 'node:http';
import net from 'node:net';
import { randomBytes, timingSafeEqual } from 'node:crypto';
import { detectTailscaleIPv4 } from '../tailscaleDetect.js';
import { isIP } from 'node:net';
import { resolveBindHost } from '../bindHost.js';
import { effectiveGatewayUrl } from '../gatewaySettingsStore.js';

export interface PreviewInfo {
  profile: string;
  sessionId: string;
  /** Port the dev server listens on inside the node (127.0.0.1). */
  devPort: number;
  /** Dedicated port this proxy listens on for the session. */
  listenPort: number;
  /** Browser-facing URL (tailscale IP when available), including the
   * listener's access secret as the PREVIEW_SECRET_PARAM query parameter. */
  url: string;
}

interface PreviewEntry extends PreviewInfo {
  server: http.Server;
  /** Access secret for this listener; stable across re-points. */
  secret: string;
}

/** Query parameter that carries a preview's access secret on first visit. */
export const PREVIEW_SECRET_PARAM = '__gah_preview';
const PREVIEW_COOKIE_PREFIX = '__gah_preview_';

export interface PreviewProxyOptions {
  /** First port of the listen range (tests override). */
  basePort?: number;
  /** Inclusive last port of the listen range. */
  maxPort?: number;
  /** Advertised host in preview URLs (tests override). */
  advertiseHost?: string;
  /** Address preview listeners bind to. Defaults to the server's HOST, so an
   * operator who binds the API to loopback does not get previews on 0.0.0.0. */
  bindHost?: string;
  /** Local ports a preview must never target (the control plane itself). */
  protectedPorts?: number[];
}

export class PreviewTargetRefusedError extends Error {}

/** Marks proxied traffic as forwarded. The control plane grants owner access
 * to loopback requests without a token, and the proxy's upstream connection
 * comes from loopback; an X-Forwarded-For header makes it refuse that trust
 * for anything that reaches it through a preview port. */
function forwardedFor(req: http.IncomingMessage): string {
  const client = req.socket.remoteAddress ?? 'unknown';
  const prior = req.headers['x-forwarded-for'];
  return prior ? `${Array.isArray(prior) ? prior.join(', ') : prior}, ${client}` : client;
}

const DEFAULT_BASE_PORT = 41_000;
const DEFAULT_MAX_PORT = 41_099;
const DEFAULT_GATEWAY_PORT = 8420;

/** Browsers do not separate cookies by port, so each listener gets its own
 * cookie name. */
function previewCookieName(listenPort: number): string {
  return `${PREVIEW_COOKIE_PREFIX}${listenPort}`;
}

function secretMatches(candidate: string | undefined, secret: string): boolean {
  if (candidate === undefined) return false;
  const a = Buffer.from(candidate);
  const b = Buffer.from(secret);
  return a.length === b.length && timingSafeEqual(a, b);
}

function cookieValue(header: string | undefined, name: string): string | undefined {
  for (const part of (header ?? '').split(';')) {
    const eq = part.indexOf('=');
    if (eq >= 0 && part.slice(0, eq).trim() === name) return part.slice(eq + 1).trim();
  }
  return undefined;
}

/** The Cookie header forwarded upstream, without any preview access cookie:
 * the dev server never needs one, and the browser sends every listener's
 * cookie to every port on the same host. */
function upstreamCookie(header: string | undefined): string | undefined {
  const kept = (header ?? '').split(';').map((part) => part.trim())
    .filter((part) => part && !part.startsWith(PREVIEW_COOKIE_PREFIX));
  return kept.length > 0 ? kept.join('; ') : undefined;
}

type PreviewAccess =
  | { kind: 'cookie'; path: string }
  /** A valid secret in the query; `path` is the request path without it. */
  | { kind: 'query'; path: string }
  | { kind: 'denied' };

/** Decides whether a request to a preview listener may reach upstream. */
function previewAccess(req: http.IncomingMessage, entry: PreviewEntry): PreviewAccess {
  const raw = req.url ?? '/';
  const queryAt = raw.indexOf('?');
  const pathname = queryAt < 0 ? raw : raw.slice(0, queryAt);
  const params = new URLSearchParams(queryAt < 0 ? '' : raw.slice(queryAt + 1));
  const offered = params.getAll(PREVIEW_SECRET_PARAM);
  if (offered.length > 0) {
    if (offered.length !== 1 || !secretMatches(offered[0], entry.secret)) return { kind: 'denied' };
    params.delete(PREVIEW_SECRET_PARAM);
    // Only a same-origin absolute path, never a protocol-relative one.
    const safePath = /^\/(?![/\\])/.test(pathname) ? pathname : '/';
    const rest = params.toString();
    return { kind: 'query', path: rest ? `${safePath}?${rest}` : safePath };
  }
  const cookie = cookieValue(req.headers.cookie, previewCookieName(entry.listenPort));
  return secretMatches(cookie, entry.secret) ? { kind: 'cookie', path: raw } : { kind: 'denied' };
}

/** The memory gateway's port: a product service, never a dev server. */
function gatewayPort(): number {
  try {
    const port = Number.parseInt(new URL(effectiveGatewayUrl()).port, 10);
    return Number.isInteger(port) && port > 0 ? port : DEFAULT_GATEWAY_PORT;
  } catch {
    return DEFAULT_GATEWAY_PORT;
  }
}

/** True when something accepts TCP connections on the loopback at `port`.
 * Auto-detection only adopts a port that is listening, not every port
 * number that appears in tool output. */
export async function loopbackPortListening(port: number, timeoutMs = 500): Promise<boolean> {
  const probe = (host: string): Promise<boolean> => new Promise((resolve) => {
    const socket = net.connect({ port, host });
    const finish = (listening: boolean): void => {
      socket.destroy();
      resolve(listening);
    };
    socket.setTimeout(timeoutMs, () => finish(false));
    socket.once('connect', () => finish(true));
    socket.once('error', () => finish(false));
  });
  return (await probe('127.0.0.1')) || (await probe('::1'));
}

function toInfo(entry: PreviewEntry): PreviewInfo {
  const { server: _server, secret: _secret, ...info } = entry;
  return info;
}

class PreviewProxy {
  private byKey = new Map<string, PreviewEntry>();
  /** In-flight set() per key: concurrent detections (tool summary AND tool
   * result in the same turn) must not each create a server -- the second
   * would leak (the existence check runs before the first registers). */
  private inflightSets = new Map<string, Promise<PreviewInfo>>();
  private options: Required<Pick<PreviewProxyOptions, 'basePort' | 'maxPort' | 'protectedPorts'>> & {
    advertiseHost?: string;
    bindHost?: string;
  } = {
    basePort: Number.parseInt(process.env.GAH_PREVIEW_BASE_PORT ?? '', 10) || DEFAULT_BASE_PORT,
    maxPort: Number.parseInt(process.env.GAH_PREVIEW_MAX_PORT ?? '', 10) || DEFAULT_MAX_PORT,
    protectedPorts: [Number.parseInt(process.env.PORT ?? '', 10) || 3773]
  };

  configure(opts: PreviewProxyOptions): void {
    if (opts.basePort !== undefined) this.options.basePort = opts.basePort;
    if (opts.maxPort !== undefined) this.options.maxPort = opts.maxPort;
    if ('advertiseHost' in opts) this.options.advertiseHost = opts.advertiseHost;
    if (opts.bindHost !== undefined) this.options.bindHost = opts.bindHost;
    if (opts.protectedPorts !== undefined) this.options.protectedPorts = opts.protectedPorts;
  }

  /** A preview may point at a dev server, never at the control plane or at
   * another preview listener (which would chain back to the same target). */
  private assertTargetAllowed(devPort: number): void {
    if (!Number.isInteger(devPort) || devPort < 1 || devPort > 65_535) {
      throw new PreviewTargetRefusedError(`Invalid preview port ${devPort}.`);
    }
    if (this.options.protectedPorts.includes(devPort)) {
      throw new PreviewTargetRefusedError(`Port ${devPort} is the control plane and cannot be previewed.`);
    }
    if (devPort >= this.options.basePort && devPort <= this.options.maxPort) {
      throw new PreviewTargetRefusedError(`Port ${devPort} is in the preview listener range and cannot be previewed.`);
    }
    if (devPort === gatewayPort()) {
      throw new PreviewTargetRefusedError(`Port ${devPort} is the memory gateway and cannot be previewed.`);
    }
  }

  private bindHost(): string {
    return this.options.bindHost ?? resolveBindHost();
  }

  /** A listener bound to one interface is only reachable at that address,
   * so the advertised URL must use it; a wildcard bind keeps the Tailscale
   * detection. */
  private async defaultAdvertiseHost(): Promise<string> {
    const bind = this.bindHost();
    if (bind === '0.0.0.0' || bind === '::' || bind === '') {
      return (await detectTailscaleIPv4()) ?? '127.0.0.1';
    }
    return isIP(bind) === 6 ? `[${bind}]` : bind;
  }

  get(profile: string, sessionId: string): PreviewInfo | null {
    const entry = this.byKey.get(`${profile}#${sessionId}`);
    return entry ? toInfo(entry) : null;
  }

  /** Lists all live previews (dashboard/debug). */
  list(): PreviewInfo[] {
    return [...this.byKey.values()].map(toInfo);
  }

  /** Points (or re-points) a session's preview at a dev port. The listen
   * port is STABLE for the session's lifetime — a dev-server restart on a
   * new port keeps the same preview URL; the proxy resolves the current
   * target per request. */
  async set(profile: string, sessionId: string, devPort: number): Promise<PreviewInfo> {
    this.assertTargetAllowed(devPort);
    const key = `${profile}#${sessionId}`;
    // Serialize per key: a concurrent set waits for the in-flight one, then
    // just re-points the (now existing) entry instead of racing a second
    // server into existence.
    const inflight = this.inflightSets.get(key);
    if (inflight) await inflight.catch(() => undefined);
    const existing = this.byKey.get(key);
    if (existing) {
      existing.devPort = devPort;
      return toInfo(existing);
    }

    const op = this.createEntry(key, profile, sessionId, devPort);
    this.inflightSets.set(key, op);
    try {
      return await op;
    } finally {
      this.inflightSets.delete(key);
    }
  }

  private async createEntry(key: string, profile: string, sessionId: string, devPort: number): Promise<PreviewInfo> {
    // The entry is created before its server so the proxy handlers resolve
    // the CURRENT devPort per request (re-points hit the same listener).
    const entry = {
      profile,
      sessionId,
      devPort,
      listenPort: 0,
      url: '',
      server: undefined as unknown as http.Server,
      secret: randomBytes(32).toString('base64url')
    } as PreviewEntry;
    entry.server = this.createProxyServer(entry);
    // Set before any further await: the access cookie's name depends on it.
    entry.listenPort = await this.listenOnFreePort(entry.server);
    const host = this.options.advertiseHost ?? (await this.defaultAdvertiseHost());
    entry.url = `http://${host}:${entry.listenPort}/?${PREVIEW_SECRET_PARAM}=${entry.secret}`;
    this.byKey.set(key, entry);
    return toInfo(entry);
  }

  async clear(profile: string, sessionId: string): Promise<void> {
    const key = `${profile}#${sessionId}`;
    // Wait out any in-flight set first, or it would register a fresh entry
    // (and listener) right after this clear removed the old one.
    const inflight = this.inflightSets.get(key);
    if (inflight) await inflight.catch(() => undefined);
    const entry = this.byKey.get(key);
    if (!entry) return;
    await this.closeEntry(entry, key);
  }

  async closeAll(): Promise<void> {
    for (const [key, entry] of [...this.byKey.entries()]) {
      await this.closeEntry(entry, key);
    }
  }

  private async closeEntry(entry: PreviewEntry, key: string): Promise<void> {
    this.byKey.delete(key);
    await new Promise<void>((resolve) => {
      entry.server.close(() => resolve());
      // close() waits for open connections; force-destroy idle keep-alive
      // sockets so a closed preview actually releases its port.
      entry.server.closeAllConnections?.();
      entry.server.closeIdleConnections?.();
    });
  }

  private createProxyServer(entry: PreviewEntry): http.Server {
    const server = http.createServer((req, res) => {
      const access = previewAccess(req, entry);
      if (access.kind === 'denied') {
        req.resume();
        res.writeHead(403, { 'content-type': 'text/plain', 'cache-control': 'no-store' });
        res.end('preview access denied: open this preview from Parasaur');
        return;
      }
      if (access.kind === 'query') {
        // Exchange the URL secret for a cookie and drop it from the address,
        // so it stays out of history and Referer headers.
        req.resume();
        res.writeHead(302, {
          location: access.path,
          'set-cookie': `${previewCookieName(entry.listenPort)}=${entry.secret}; Path=/; HttpOnly; SameSite=Lax`,
          'cache-control': 'no-store',
          'referrer-policy': 'no-referrer'
        });
        res.end();
        return;
      }
      const devPort = entry.devPort;
      const headers: http.OutgoingHttpHeaders = { ...req.headers, host: `localhost:${devPort}`, 'x-forwarded-for': forwardedFor(req) };
      const cookie = upstreamCookie(req.headers.cookie);
      if (cookie) headers.cookie = cookie;
      else delete headers.cookie;
      // agent:false — one connection per request. The global agent pools
      // keep-alive sockets to the dev server that outlive preview clears
      // (and pin the process open in tests); preview traffic doesn't need
      // the reuse.
      const proxied = http.request(
        {
          host: '127.0.0.1',
          port: devPort,
          method: req.method,
          path: access.path,
          headers,
          agent: false
        },
        (upstream) => {
          res.writeHead(upstream.statusCode ?? 502, upstream.headers);
          upstream.pipe(res);
        }
      );
      proxied.on('error', () => {
        if (!res.headersSent) res.writeHead(502, { 'content-type': 'text/plain' });
        res.end(`preview target unreachable: is the dev server running on port ${devPort}?`);
      });
      req.pipe(proxied);
    });

    // WebSocket upgrade (Vite HMR): splice a raw TCP socket to the dev
    // server with the Host header rewritten. No frame inspection needed.
    server.on('upgrade', (req, socket, head) => {
      // An upgrade cannot be redirected: a secret in its query is accepted
      // and stripped, like the cookie.
      const access = previewAccess(req, entry);
      if (access.kind === 'denied') {
        socket.destroy();
        return;
      }
      const devPort = entry.devPort;
      const upstream = net.connect(devPort, '127.0.0.1', () => {
        const lines = [`${req.method} ${access.path} HTTP/${req.httpVersion}`];
        for (const [name, value] of Object.entries(req.headers)) {
          if (name === 'host') {
            lines.push(`host: localhost:${devPort}`);
          } else if (name === 'x-forwarded-for') {
            continue;
          } else if (name === 'cookie') {
            const cookie = upstreamCookie(Array.isArray(value) ? value.join('; ') : value);
            if (cookie) lines.push(`cookie: ${cookie}`);
          } else if (Array.isArray(value)) {
            for (const v of value) lines.push(`${name}: ${v}`);
          } else if (value !== undefined) {
            lines.push(`${name}: ${value}`);
          }
        }
        lines.push(`x-forwarded-for: ${forwardedFor(req)}`);
        upstream.write(`${lines.join('\r\n')}\r\n\r\n`);
        if (head.length > 0) upstream.write(head);
        socket.pipe(upstream);
        upstream.pipe(socket);
      });
      upstream.on('error', () => socket.destroy());
      socket.on('error', () => upstream.destroy());
    });

    return server;
  }

  private listenOnFreePort(server: http.Server): Promise<number> {
    const candidates: number[] = [];
    const used = new Set([...this.byKey.values()].map((e) => e.listenPort));
    for (let port = this.options.basePort; port <= this.options.maxPort; port++) {
      if (!used.has(port)) candidates.push(port);
    }
    if (candidates.length === 0) {
      return Promise.reject(new Error('preview port range exhausted'));
    }
    return new Promise<number>((resolve, reject) => {
      const tryNext = (index: number): void => {
        if (index >= candidates.length) {
          reject(new Error('no free port in the preview range'));
          return;
        }
        const port = candidates[index];
        // Each attempt removes the other handler, or a failed attempt's
        // 'listening' callback would resolve with its occupied port.
        const onListening = (): void => {
          server.off('error', onError);
          resolve(port);
        };
        const onError = (): void => {
          server.off('listening', onListening);
          tryNext(index + 1);
        };
        server.once('error', onError);
        server.once('listening', onListening);
        server.listen(port, this.bindHost());
      };
      tryNext(0);
    });
  }
}

export const previewProxy = new PreviewProxy();

/**
 * Auto-detects a dev-server port from agent tool output (the slice-3
 * structured stream). Matches the shapes the common dev servers print:
 * "Local: http://localhost:5173/", "listening on 127.0.0.1:3000",
 * "running at http://[::1]:8080", "Server running on port 4200".
 * Returns the LAST match (dev servers restart and print again).
 */
export function detectDevPort(text: string): number | null {
  const patterns = [
    /(?:localhost|127\.0\.0\.1|\[::1\]):(\d{2,5})/gi,
    /\bport\s+(\d{2,5})\b/gi
  ];
  let port: number | null = null;
  for (const pattern of patterns) {
    for (const match of text.matchAll(pattern)) {
      const candidate = Number.parseInt(match[1], 10);
      if (candidate >= 1024 && candidate <= 65_535) port = candidate;
    }
  }
  return port;
}
