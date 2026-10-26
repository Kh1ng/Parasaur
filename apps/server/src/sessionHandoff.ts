import crypto from 'node:crypto';
import { Router } from 'express';
import rateLimit from 'express-rate-limit';
import { authMiddleware, isTrustedLocalRequest, requireOwner, sameOriginRequest } from './authMiddleware.js';

const hash = (value: string) => crypto.createHash('sha256').update(value).digest('hex');
const validSecret = (value: unknown): value is string => typeof value === 'string' && /^[A-Za-z0-9_-]{43}$/.test(value);

/** A sign-in tab can approve a code, but only its initiating iframe knows the
 * verifier needed to claim it. Nothing persists; memory holds only hashes.
 * Claims consume the approval synchronously and expire after sixty seconds. */
export function sessionHandoffRouter(now = Date.now): Router {
  const router = Router();
  const pending = new Map<string, { verifierHash: string; origin: string; expires: number; approvedHash?: string }>();
  router.use(rateLimit({ windowMs: 60_000, limit: 60, standardHeaders: true, legacyHeaders: false }));
  router.use((req, res, next) => {
    res.set('Cache-Control', 'no-store');
    if ((!req.secure && !isTrustedLocalRequest(req)) || !sameOriginRequest(req)) {
      return res.status(403).json({ message: 'Session handoff requires the same server origin and TLS.' });
    }
    for (const [key, entry] of pending) if (entry.expires <= now()) pending.delete(key);
    next();
  });
  router.post('/', (req, res) => {
    if (pending.size >= 100) return res.status(429).json({ message: 'Too many pending sign-ins. Retry in a minute.' });
    const code = crypto.randomBytes(32).toString('base64url');
    const verifier = crypto.randomBytes(32).toString('base64url');
    pending.set(hash(code), { verifierHash: hash(verifier), origin: new URL(`${req.protocol}://${req.headers.host}`).origin,
      expires: now() + 60_000 });
    res.json({ code, verifier, expires_in: 60 });
  });
  router.post('/approve', authMiddleware, requireOwner, (req, res) => {
    const code = req.body?.code;
    const entry = validSecret(code) ? pending.get(hash(code)) : undefined;
    if (!entry || entry.origin !== new URL(`${req.protocol}://${req.headers.host}`).origin || entry.approvedHash) {
      return res.status(410).json({ message: 'Sign-in link expired or already used. Reopen it from the panel.' });
    }
    if (!process.env.COORDINATOR_TOKEN) return res.status(503).json({ message: 'Owner token is not configured.' });
    entry.approvedHash = hash(process.env.COORDINATOR_TOKEN);
    res.json({ approved: true });
  });
  router.post('/claim', (req, res) => {
    const { code, verifier } = req.body ?? {};
    const entry = validSecret(code) ? pending.get(hash(code)) : undefined;
    if (!entry || !validSecret(verifier) || entry.verifierHash !== hash(verifier)
      || entry.origin !== new URL(`${req.protocol}://${req.headers.host}`).origin) {
      return res.status(410).json({ message: 'Sign-in expired or belongs to another panel.' });
    }
    if (!entry.approvedHash) return res.status(202).json({ pending: true });
    pending.delete(hash(code));
    if (!process.env.COORDINATOR_TOKEN || entry.approvedHash !== hash(process.env.COORDINATOR_TOKEN)) {
      return res.status(410).json({ message: 'Owner session changed. Reopen the sign-in link from the panel.' });
    }
    res.json({ token: process.env.COORDINATOR_TOKEN });
  });
  return router;
}
