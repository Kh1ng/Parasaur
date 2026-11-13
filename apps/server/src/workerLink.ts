/**
 * Worker links: how this central reaches the workers registered with it.
 *
 * A worker registers one address. Everything the central later sends to that
 * worker (health polls, quota reads, native dispatch, chat, updates, login
 * repair) goes through `linkTo(node)`, which settles three things in one place:
 *
 *   1. What kind of path the address is. The kind is derived from the address
 *      alone. Nobody declares it, so it cannot disagree with the address.
 *   2. Whether this central may send over that kind of path.
 *   3. Which credential proves the central to that worker.
 *
 * Callers name a worker and a path. They never read `advertised_url`,
 * `transport_mode`, `secret_ref` or GAH_ALLOW_INSECURE_HTTP to build a request.
 *
 * Kinds, and when this central accepts them:
 *
 *   loopback  The same machine, or the near end of an SSH tunnel. Always.
 *   https     Any TLS origin: a reverse proxy or Tailscale Serve today.
 *             Always.
 *   tailnet   Plain HTTP to a Tailscale address. WireGuard encrypts it.
 *             Only when PARASAUR_WORKER_LINKS lists `tailnet`.
 *   lan       Plain HTTP to any other address, on a network the operator
 *             trusts. Only when PARASAUR_WORKER_LINKS lists `lan`.
 *
 * GAH_ALLOW_INSECURE_HTTP=1 predates this module and still enables both
 * plain-HTTP kinds, so an existing deployment keeps working unchanged.
 *
 * Planned: a hosted central reaches a worker through a Cloudflare Tunnel. That
 * is the `https` kind plus a Cloudflare Access service token, which `headers()`
 * would add; no caller changes. See docs/WORKER_LINKS.md.
 */
import { readFileSync } from 'node:fs';
import { isIP } from 'node:net';
import { resolve, sep } from 'node:path';
import { COORDINATOR_VERSION, type RegisteredNode, type WorkerLinkKind } from '@git-agent-harness/contracts';

/** This central will not use the worker's address. The message names the
 * address, the reason, and the setting or registration that would change the
 * answer. It holds no secret and is safe to show an operator. */
export class LinkRefused extends Error {}

/** A request to a worker. The link owns the destination, the credential and
 * the redirect rule, so a caller cannot override them. */
export type WorkerRequest = Omit<RequestInit, 'headers' | 'redirect'> & { headers?: Record<string, string> };

export interface WorkerLink {
  readonly kind: WorkerLinkKind;
  /** Scheme, host and port that every request on this link goes to. */
  readonly origin: string;
  /** Headers that identify this central to the worker. Throws when the
   * worker's credential cannot be read on this machine. */
  headers(): Record<string, string>;
  /** Sends one request to `path` on the worker and returns its reply. A
   * redirect is an error, so a reply can never carry the credential to
   * another host. `via` replaces the network call in tests. */
  fetch(path: string, init?: WorkerRequest, via?: typeof fetch): Promise<Response>;
}

/** The fields of a registration that decide its link. */
export type LinkRegistration = Pick<RegisteredNode, 'advertised_url' | 'transport_mode' | 'secret_ref'>;

/** The link to a registered worker. Throws `LinkRefused` when this central
 * will not send to the worker's address. Reads no secret and does no I/O, so
 * registration and every request can call it. */
export function linkTo(node: LinkRegistration): WorkerLink {
  let url: URL;
  try { url = new URL(node.advertised_url); }
  catch { throw new LinkRefused(`Worker address '${node.advertised_url}' is not a URL.`); }
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password) {
    throw new LinkRefused(`Worker address '${url.origin}' must be an http:// or https:// address without credentials.`);
  }
  const kind = kindOf(url);
  const origin = url.origin;
  // `transport_mode` is the registrant's own claim about its address. It
  // decides nothing here, but a claim the address contradicts is refused.
  if (node.transport_mode === 'loopback' && kind !== 'loopback') {
    throw new LinkRefused('Non-loopback advertised URL cannot use loopback transport mode');
  }
  if (node.transport_mode === 'authenticated_remote' && (kind === 'tailnet' || kind === 'lan')) {
    throw new LinkRefused('Non-loopback authenticated remote endpoints must use TLS (https:// or wss://)');
  }
  if (!linkEnabled(kind)) {
    throw new LinkRefused(`Worker address ${origin} is plain HTTP on a ${kind === 'tailnet' ? 'tailnet' : 'local network'}, and this central does not accept ${kind} links. `
      + `Add '${kind}' to PARASAUR_WORKER_LINKS in the central server's environment, or register an HTTPS address.`);
  }
  // A worker on this machine trusts local callers. Sending it a credential it
  // was never given would turn a working request into a rejected one.
  const identifies = kind !== 'loopback' || node.transport_mode === 'authenticated_remote';
  const headers = () => {
    const sent: Record<string, string> = { Accept: 'application/json', 'User-Agent': `GAH-Coordinator/${COORDINATOR_VERSION}` };
    if (identifies) {
      const token = resolveSecret(node.secret_ref);
      if (!token || /[\x00-\x1f\x7f]/.test(token)) throw new Error('Worker credential is empty or malformed');
      sent.Authorization = `Bearer ${token}`;
    }
    return sent;
  };
  return {
    kind, origin, headers,
    async fetch(path, init = {}, via = fetch) {
      const target = new URL(path, origin);
      if (target.origin !== origin) throw new LinkRefused(`Request path '${path}' leaves worker address ${origin}.`);
      return via(target, { ...init, redirect: 'error', headers: { ...headers(), ...init.headers } });
    },
  };
}

/** Whether this central accepts workers over `kind`. For setup flows that
 * must refuse before producing a worker the central could not reach. */
export function linkEnabled(kind: WorkerLinkKind): boolean {
  if (kind === 'loopback' || kind === 'https') return true;
  return process.env.GAH_ALLOW_INSECURE_HTTP === '1'
    || (process.env.PARASAUR_WORKER_LINKS ?? '').split(/[\s,]+/).includes(kind);
}

function kindOf(url: URL): WorkerLinkKind {
  const host = url.hostname.replace(/^\[|\]$/g, '').toLowerCase();
  if (host === 'localhost' || host === '::1' || (isIP(host) === 4 && host.startsWith('127.'))) return 'loopback';
  if (url.protocol === 'https:') return 'https';
  return isTailnetHost(host) ? 'tailnet' : 'lan';
}

/** Tailscale's own ranges: 100.64.0.0/10, fd7a:115c:a1e0::/48, and MagicDNS names. */
function isTailnetHost(host: string): boolean {
  if (isIP(host) === 4) {
    const [first, second] = host.split('.').map(Number);
    return first === 100 && second >= 64 && second <= 127;
  }
  return isIP(host) === 6 ? host.startsWith('fd7a:115c:a1e0:') : host.endsWith('.ts.net');
}

/** Registrants choose `advertised_url`, so a `file:` reference must never
 * resolve outside a known directory. Otherwise a registrant could point its
 * address at a server it controls and have the central read any file its
 * process can, then send the contents there as the Authorization header. */
const DEFAULT_NODE_SECRETS_ROOT = '/etc/gah/node-secrets';

function nodeSecretsRoot(): string {
  return resolve(process.env.GAH_NODE_SECRETS_ROOT || DEFAULT_NODE_SECRETS_ROOT);
}

/** Reads the credential a registration names, as `env:NAME` or `file:/path`. */
export function resolveSecret(secretRef: string): string {
  if (!secretRef) {
    throw new Error('Secret reference is empty');
  }
  if (secretRef.startsWith('env:')) {
    const envVar = secretRef.slice(4);
    const val = process.env[envVar];
    if (val === undefined) {
      throw new Error(`Environment variable ${envVar} is not set`);
    }
    return val;
  }
  if (secretRef.startsWith('file:')) {
    const root = nodeSecretsRoot();
    const filePath = resolve(secretRef.slice(5));
    if (filePath !== root && !filePath.startsWith(root + sep)) {
      throw new Error(`Secret file path must be inside ${root}`);
    }
    try {
      return readFileSync(filePath, 'utf8').trim();
    } catch (e: any) {
      throw new Error(`Failed to read secret file ${filePath}: ${e.message}`);
    }
  }
  throw new Error(`Unsupported secret reference format (must start with 'env:' or 'file:')`);
}
