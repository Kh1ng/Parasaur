import { homedir } from 'node:os';
import { resolve } from 'node:path';
import { Router } from 'express';
import rateLimit from 'express-rate-limit';
import { isLocalAddress } from '../../authMiddleware.js';
import { classifyEvent, type GitLabEvent } from './classifier.js';
import { EventDedupe } from './dedupe.js';
import { dispatchGitLabEvent } from './dispatcher.js';
import { verifyHookSecret } from './verifier.js';

export type GitLabRouteDeps = {
  dedupe?: EventDedupe;
  dispatch?: (event: GitLabEvent, options: { bot: string; panelUrl: string }) => Promise<void>;
};

/** Hook credentials authorize only this bounded, asynchronous ingestion route. */
export function gitLabEventsRouter(deps: GitLabRouteDeps = {}): Router {
  const router = Router();
  let pending = 0;
  const dedupe = deps.dedupe ?? new EventDedupe(10_000, 86_400_000, Date.now,
    process.env.GAH_GITLAB_EVENTS_STORE_PATH || resolve(process.env.XDG_STATE_HOME || resolve(homedir(), '.local/state'), 'gah/gitlab-events.json'));
  const active = new Set<string>();
  const drain = () => {
    try {
      for (const [key, queued] of dedupe.queued()) {
        if (active.has(key) || pending >= 64) continue;
        const event = classifyEvent(queued.body, queued.bot, queued.pipelineEnabled, queued.eventUuid);
        if (!event || event.key !== key) continue;
        active.add(key);
        pending++;
        void Promise.resolve().then(() => (deps.dispatch ?? dispatchGitLabEvent)(event, queued)).then(() => {
          // Fleet start resolves after recording its durable request-ID lease.
          dedupe.complete(key);
        }).catch(() => {
          console.error(`[gitlab-events] Event ${key} remains queued for retry.`);
        }).finally(() => { active.delete(key); pending--; });
      }
    } catch {
      console.error('[gitlab-events] Event store unavailable; queued events retained.');
    }
  };
  setImmediate(drain);
  const retry = setInterval(drain, 30_000);
  retry.unref();
  const configuredLimit = Number(process.env.GAH_GITLAB_EVENTS_RATE_LIMIT);
  const limit = Number.isSafeInteger(configuredLimit) && configuredLimit > 0 ? configuredLimit : 600;
  router.use(rateLimit({ windowMs: 60_000, limit, standardHeaders: true, legacyHeaders: false,
    message: { error: 'rate_limited' } }));
  router.post('/events', (req, res, next) => {
    if (!req.secure && !isLocalAddress(req.socket.remoteAddress ?? '') && process.env.GAH_ALLOW_INSECURE_HTTP !== '1') {
      return res.status(403).json({ error: 'https_required' });
    }
    if (!verifyHookSecret(req.get('X-Parasaur-Hook-Secret'))) return res.status(401).json({ error: 'authentication_required' });
    next();
  }, (req, res, next) => {
    const bot = process.env.GAH_GITLAB_BOT_USERNAME || 'parasaur';
    const event = classifyEvent(req.body, bot, process.env.GAH_GITLAB_PIPELINE_REPAIR === '1', req.get('X-Gitlab-Event-UUID'));
    if (!event) return res.sendStatus(204);
    res.locals.gitLabEvent = event;
    next();
  }, (req, res) => {
    const event: GitLabEvent = res.locals.gitLabEvent;
    const bot = process.env.GAH_GITLAB_BOT_USERNAME || 'parasaur';
    let panelUrl: string;
    try {
      const url = new URL(process.env.GAH_GITLAB_PANEL_URL || '');
      if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.search || url.hash) throw new Error('Invalid panel URL');
      panelUrl = url.href;
    } catch { return res.status(503).json({ error: 'gitlab_panel_url_required' }); }
    if (pending >= 64) return res.status(503).json({ error: 'gitlab_event_queue_full' });
    let reservation: ReturnType<EventDedupe['reserve']>;
    try { reservation = dedupe.reserve(event.key, { body: req.body, bot, panelUrl,
      pipelineEnabled: process.env.GAH_GITLAB_PIPELINE_REPAIR === '1', eventUuid: req.get('X-Gitlab-Event-UUID') }); }
    catch { return res.status(503).json({ error: 'gitlab_event_store_unavailable' }); }
    if (reservation === 'duplicate') return res.sendStatus(204);
    if (reservation === 'full') return res.status(503).json({ error: 'gitlab_event_store_full' });
    res.sendStatus(202);
    setImmediate(drain);
  });
  return router;
}
