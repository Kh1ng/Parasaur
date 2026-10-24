import type { RequestHandler } from 'express';

/** Only explicit HTTP(S) origins can grant framing; never trust query parameters. */
export function gitlabOrigins(value = process.env.GAH_GITLAB_ORIGINS ?? ''): string[] {
  return [...new Set(value.split(/[\s,]+/).filter(Boolean).flatMap(value => {
    try {
      const url = new URL(value);
      return ['https:', 'http:'].includes(url.protocol) && url.origin === value ? [url.origin] : [];
    } catch { return []; }
  }))];
}

export function forwardedPrefix(value: string | undefined): string {
  return value && /^\/(?:[A-Za-z0-9_-]+\/?)+$/.test(value) ? value.replace(/\/$/, '') : '';
}

export const framePolicy: RequestHandler = (req, res, next) => {
  const origins = gitlabOrigins();
  const embed = req.path === '/embed';
  res.setHeader('Content-Security-Policy', `frame-ancestors ${embed && origins.length ? origins.join(' ') : "'none'"}`);
  if (!embed || !origins.length) res.setHeader('X-Frame-Options', 'DENY');
  next();
};

export function webEntry(html: string, prefix: string, origins: string[]): string {
  const config = JSON.stringify({ prefix, gitlabOrigins: origins }).replace(/</g, '\\u003c');
  return html.replace(/(src|href)="\/(?!\/)/g, `$1="${prefix}/`).replace(/<head>/i, `<head><base href="${prefix}/"><script id="parasaur-config" type="application/json">${config}</script>`);
}
