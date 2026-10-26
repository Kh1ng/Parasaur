import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import express, { Router } from 'express';
import rateLimit from 'express-rate-limit';
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

/** The one pipeline every served HTML document goes through: it sets the
 * framing policy and injects the prefix bootstrap. `frameable` picks the
 * embed route's policy (CSP naming the configured origins, no
 * `X-Frame-Options`) over the app's (`X-Frame-Options: DENY`, never
 * frameable). */
function spaDocumentHandler(options: EmbedViewOptions, frameable: boolean): RequestHandler {
  const indexPath = resolve(options.webRoot, 'index.html');
  return (req: Request, res: Response, next: NextFunction) => {
    if (isApiRequest(req.path) || !req.accepts('html')) return next();
    let html: string;
    try {
      html = readFileSync(indexPath, 'utf8');
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

/** All dashboard documents share framing, bootstrap, and a per-IP read limit.
 * Static middleware owns asset path resolution; request paths never enter our
 * filesystem calls, and explicit HTML URLs always use the fixed index file. */
export function documentRouter(options: EmbedViewOptions): Router {
  const router = Router();
  const assets = express.static(options.webRoot, { index: false });
  router.use(rateLimit({ windowMs: 60_000, limit: 300, standardHeaders: true, legacyHeaders: false }));
  router.get('/embed', spaDocumentHandler(options, true));
  router.use((req, res, next) => {
    if (isApiRequest(req.path)) return next('router');
    let assetPath: string;
    try { assetPath = decodeURIComponent(req.path); } catch { return next(); }
    if (/\.html?(?:\/|$)/i.test(assetPath)) return next();
    assets(req, res, next);
  });
  router.get('*', spaDocumentHandler(options, false));
  return router;
}
