import { homedir } from 'node:os';
import { resolve } from 'node:path';
import { Router } from 'express';
import rateLimit from 'express-rate-limit';
import { getFleetDispatch } from '../../wsServer.js';
import { isLocalAddress } from '../../authMiddleware.js';
import { classifyEvent, type GitLabEvent } from './classifier.js';
import { GitLabEventStore } from './eventStore.js';
import { dispatchGitLabEvent } from './dispatcher.js';
import { GitLabEventQueue, type GitLabDispatch, type FindGitLabDispatch } from './queue.js';
import { verifyHookSecret } from './verifier.js';

export type GitLabRouteDeps = {
  store?: GitLabEventStore;
  dispatch?: GitLabDispatch;
  findDispatch?: FindGitLabDispatch;
};

/** Hook credentials authorize only this bounded, asynchronous ingestion route. */
export function gitLabEventsRouter(deps: GitLabRouteDeps = {}): Router & { queue: GitLabEventQueue; store: GitLabEventStore } {
  const router = Router();
  const store = deps.store ?? new GitLabEventStore(10_000, 86_400_000, Date.now,
    process.env.GAH_GITLAB_EVENTS_STORE_PATH || resolve(process.env.XDG_STATE_HOME || resolve(homedir(), '.local/state'), 'gah/gitlab-events.json'));
  const queue = new GitLabEventQueue(store, deps.dispatch ?? dispatchGitLabEvent,
    deps.findDispatch ?? (async identity => {
      const fleet = getFleetDispatch();
      return (identity.sessionId ? fleet.getSession(identity.sessionId) : undefined)
        ?? fleet.getSessionByRequestId(identity.requestId);
    }));
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
    } catch { return res.set('Retry-After', '30').status(503).json({ error: 'gitlab_panel_url_required' }); }
    let reservation: ReturnType<GitLabEventStore['enqueue']>;
    try {
      reservation = store.enqueue({ key: event.key, body: req.body, eventUuid: req.get('X-Gitlab-Event-UUID'),
        bot, pipelineEnabled: process.env.GAH_GITLAB_PIPELINE_REPAIR === '1', panelUrl });
    }
    catch { return res.set('Retry-After', '30').status(503).json({ error: 'gitlab_event_store_unavailable' }); }
    if (reservation === 'duplicate') return res.sendStatus(204);
    if (reservation === 'queue_full') return res.set('Retry-After', String(store.retryAfter())).status(503).json({ error: 'gitlab_event_queue_full' });
    if (reservation === 'full') return res.set('Retry-After', String(store.retryAfter())).status(503).json({ error: 'gitlab_event_store_full' });
    res.sendStatus(202);
    queue.wake();
  });
  return Object.assign(router, { queue, store });
}
