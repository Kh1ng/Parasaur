import { readFileSync, statSync } from 'node:fs';
import { join, resolve, sep } from 'node:path';
import type { NextFunction, Request, RequestHandler, Response } from 'express';

/**
 * GitLab panel embed view (#1593): the chrome-less `/embed` route the
 * parasaur-gitlab side panel iframes, plus the framing policy for the rest
 * of the dashboard. The panel contract lives in the companion repo's
 * docs/parasaur-contract.md; this module owns the server side:
 *
 * - `/embed` HTML is frameable only by the origins configured through
 *   `GAH_GITLAB_ORIGIN` (space- or comma-separated http(s) origins). With
 *   no valid origin configured the route fails closed: `frame-ancestors
 *   'none'`, so no site may frame it.
 * - Every other served HTML document keeps `X-Frame-Options: DENY`, so
 *   the rest of the app stays non-frameable.
 * - Served documents carry the public base path derived from
 *   `X-Forwarded-Prefix` (nginx proxies Parasaur under `/parasaur/`) and
 *   the configured GitLab origins, so the SPA builds asset, API, and
 *   WebSocket URLs under the prefix and validates panel postMessage
 *   against the same configured origins.
 */

/** Maximum distinct origins one server accepts framing from. */
const MAX_FRAME_ORIGINS = 8;

/** Origins allowed to frame the embed view: `GAH_GITLAB_ORIGIN`, split on
 * whitespace or commas. Each entry must be a bare http(s) origin — exactly
 * `http:` or `https:`, no wildcard anywhere (an entry containing `*` would
 * become an active CSP wildcard, not an exact origin), no credentials, no
 * path. Invalid entries are dropped, and with none left the route fails
 * closed. */
export function gitLabFrameOrigins(raw: string | undefined = process.env.GAH_GITLAB_ORIGIN): string[] {
  const valid: string[] = [];
  for (const entry of (raw ?? '').split(/[\s,]+/)) {
    if (entry.length === 0 || valid.length >= MAX_FRAME_ORIGINS) continue;
    // `https://*` and `https://*.example.com` parse but are wildcards: a
    // frame-ancestors entry must name one origin, never a pattern.
    if (entry.includes('*')) continue;
    let url: URL;
    try {
      url = new URL(entry);
    } catch {
      continue;
    }
    // Exactly http/https — `startsWith('http')` would admit `httpx://`,
    // whose origin serializes as the string 'null'.
    const bareOrigin = (url.protocol === 'http:' || url.protocol === 'https:') && !url.username && !url.password
      && url.pathname === '/' && url.search === '' && url.hash === '' && !url.hostname.includes('*');
    if (bareOrigin && !valid.includes(url.origin)) valid.push(url.origin);
  }
  return valid;
}

/** The `Content-Security-Policy` framing directive for the embed route. */
export function framePolicyHeader(origins: string[]): string {
  return origins.length > 0 ? `frame-ancestors ${origins.join(' ')}` : "frame-ancestors 'none'";
}

/** The public base path from `X-Forwarded-Prefix`: `/parasaur` (no trailing
 * slash) or '' when absent or malformed. Only plain path segments pass, so
 * an injected `//host` or `/..` cannot smuggle another origin into URLs. */
export function publicPathPrefix(header: string | undefined): string {
  const trimmed = (header ?? '').trim();
  if (trimmed.length === 0 || trimmed.length > 128) return '';
  if (!/^\/[A-Za-z0-9._~-]+(?:\/[A-Za-z0-9._~-]+)*\/?$/.test(trimmed)) return '';
  if (trimmed.split('/').some(segment => segment === '.' || segment === '..')) return '';
  return trimmed.replace(/\/+$/, '');
}

function escapeHtmlAttribute(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
}

/** Serialize configuration for the SPA's bootstrap script. `<` is escaped
 * so no configured value can close the script tag early. */
export function bootstrapScript(basePath: string, gitLabOrigins: string[]): string {
  const config = JSON.stringify({ basePath, gitLabOrigins }).replace(/</g, '\\u003c');
  const base = basePath ? `<base href="${escapeHtmlAttribute(`${basePath}/`)}">` : '';
  return `${base}<script>window.__GAH_BOOTSTRAP__=${config};</script>`;
}

function injectedDocument(html: string, basePath: string, origins: string[]): string {
  const head = bootstrapScript(basePath, origins);
  return head && html.includes('</head>') ? html.replace('</head>', `${head}</head>`) : html;
}

export type EmbedViewOptions = {
  /** Built web app root (GAH_WEB_ROOT), already resolved by the caller. */
  webRoot: string;
  /** Configuration source; read per request so tests can flip it live. */
  env?: { GAH_GITLAB_ORIGIN?: string };
};

/** `GAH_GITLAB_ORIGIN` from the injected env, or the process environment. */
function configuredOrigin(options: EmbedViewOptions): string | undefined {
  return options.env ? options.env.GAH_GITLAB_ORIGIN : process.env.GAH_GITLAB_ORIGIN;
}

/** The API boundary: requests under it never fall through to the SPA. */
export function isApiRequest(path: string): boolean {
  return path.startsWith('/api/') || path === '/api';
}

/** True when the path maps to an existing file in the web root that is not
 * an HTML document — a static asset (`express.static` serves it as-is).
 * HTML files (`/index.html`) are documents and never static assets: every
 * HTML-serving path goes through the document pipeline below, so no served
 * HTML document can bypass its framing policy or the prefix bootstrap. */
export function isStaticAsset(webRoot: string, path: string): boolean {
  if (path === '' || path === '/') return false;
  const root = resolve(webRoot);
  const target = resolve(root, `.${path}`);
  if (target !== root && !target.startsWith(root + sep)) return false;
  let file: { isFile(): boolean } | null = null;
  try {
    file = statSync(target);
  } catch {
    return false;
  }
  return file.isFile() && !/\.html?$/i.test(target);
}

/** The one pipeline every served HTML document goes through: it sets the
 * framing policy and injects the prefix bootstrap. `frameable` picks the
 * embed route's policy (CSP naming the configured origins, no
 * `X-Frame-Options`) over the app's (`X-Frame-Options: DENY`, never
 * frameable). */
function spaDocumentHandler(options: EmbedViewOptions, frameable: boolean): RequestHandler {
  const webRoot = resolve(options.webRoot);
  return (req: Request, res: Response, next: NextFunction) => {
    if (isApiRequest(req.path) || !req.accepts('html')) return next();
    let html: string;
    try {
      html = readFileSync(join(webRoot, 'index.html'), 'utf8');
    } catch (error) {
      return next(error);
    }
    const origins = gitLabFrameOrigins(configuredOrigin(options));
    if (frameable) res.setHeader('Content-Security-Policy', framePolicyHeader(origins));
    else res.setHeader('X-Frame-Options', 'DENY');
    res.setHeader('Cache-Control', 'no-store');
    res.send(injectedDocument(html, publicPathPrefix(req.get('X-Forwarded-Prefix')), origins));
  };
}

/** Serve the SPA document for the embed route: frameable only by the
 * configured GitLab origins, no `X-Frame-Options`. */
export function embedHtmlHandler(options: EmbedViewOptions): RequestHandler {
  return spaDocumentHandler(options, true);
}

/** Serve the SPA document for every other route — `/`, `/index.html`, and
 * the SPA fallback alike, always through the document pipeline above —
 * never frameable. Existing non-HTML files (assets) fall through so static
 * middleware serves them. */
export function appHtmlHandler(options: EmbedViewOptions): RequestHandler {
  const webRoot = resolve(options.webRoot);
  const handler = spaDocumentHandler(options, false);
  return (req: Request, res: Response, next: NextFunction) => {
    if (isStaticAsset(webRoot, req.path)) return next();
    handler(req, res, next);
  };
}
