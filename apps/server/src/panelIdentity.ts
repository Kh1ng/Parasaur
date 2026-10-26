import { sameOriginRequest } from './authMiddleware.js';
import { createHash, randomUUID } from 'node:crypto';
import type { IncomingMessage } from 'node:http';
import type { RequestHandler } from 'express';
import { appendFileSync, mkdirSync, openSync, fsyncSync, closeSync } from 'node:fs';
import { resolve, join } from 'node:path';

/** Stable provider subject, never a display name or a client-supplied actor. */
export interface PanelIdentity {
  issuer: string;
  subject: string;
  canApprovePaidRoutes: boolean;
}

/** Enterprise adapters must verify a session/token or an authenticated proxy
 * channel. Reading GitLab identity headers alone is not verification.
 * null means unauthenticated; errors fail closed. No adapter is enabled by default.
 */
export interface PanelIdentityProvider {
  authenticate(request: IncomingMessage): Promise<PanelIdentity | null>;
}

export async function resolvePanelIdentity(provider: PanelIdentityProvider, request: IncomingMessage): Promise<PanelIdentity | null> {
  const identity = await provider.authenticate(request);
  if (identity === null) return null;
  const valid = (value: unknown) => typeof value === 'string' && value.trim() === value && value.length > 0 && value.length <= 512 && !/[\x00-\x1f\x7f]/.test(value);
  if (!valid(identity.issuer) || !valid(identity.subject) || typeof identity.canApprovePaidRoutes !== 'boolean') throw new Error('Invalid panel identity');
  return { issuer: identity.issuer, subject: identity.subject, canApprovePaidRoutes: identity.canApprovePaidRoutes };
}

export function panelActor(identity: PanelIdentity): string {
  return `panel:${JSON.stringify([identity.issuer, identity.subject])}`;
}

/** Attribution lives in a session artifact, without credentials or chat text.
 * Refuse to start work when its attribution cannot be persisted. */
export function recordPanelAction(identity: PanelIdentity, action: string, requestId?: string, target?: unknown): void {
  const directory = process.env.GAH_MUTATION_STORE_PATH ?? resolve('config/mutations');
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const file = openSync(join(directory, 'panel-actions.jsonl'), 'a', 0o600);
  try {
    appendFileSync(file, `${JSON.stringify({ schema_version: 1, timestamp: new Date().toISOString(), actor: panelActor(identity), action, request_id: requestId, target_digest: target === undefined ? undefined : createHash('sha256').update(JSON.stringify(target)).digest('hex') })}\n`);
    fsyncSync(file);
  } finally { closeSync(file); }
  const folder = openSync(directory, 'r');
  try { fsyncSync(folder); } finally { closeSync(folder); }
}

export function panelIdentityMiddleware(provider?: PanelIdentityProvider): RequestHandler {
  return async (req, res, next) => {
    if (!provider) return next();
    if ((req.headers.cookie !== undefined || req.headers.origin !== undefined) && !sameOriginRequest(req)) {
      return res.status(403).json({ error: 'panel_origin_forbidden' });
    }
    try {
      const identity = await resolvePanelIdentity(provider, req);
      if (!identity) return res.status(401).json({ error: 'panel_identity_required' });
      res.locals.authPrincipal = { kind: 'panel', identity };
      if (!['GET', 'HEAD', 'OPTIONS'].includes(req.method)) {
        const requestId = randomUUID();
        res.setHeader('X-GAH-Identity-Request-Id', requestId);
        recordPanelAction(identity, `${req.method} ${req.path}`, requestId, req.body ?? null);
      }
      next();
    } catch { return res.status(503).json({ error: 'panel_identity_unavailable' }); }
  };
}
