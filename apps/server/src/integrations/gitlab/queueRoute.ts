import { Router } from 'express';
import { requireOwner } from '../../authMiddleware.js';
import type { mutationSafety } from '../../mutationSafety.js';
import type { GitLabEventStore } from './eventStore.js';
import type { GitLabEventQueue } from './queue.js';

/** Owner recovery routes, mounted after the shared API authentication and node policy. */
export function gitLabQueueRouter(store: GitLabEventStore, queue: GitLabEventQueue,
  mutation: ReturnType<typeof mutationSafety>): Router {
  const router = Router();
  router.use('/queue', requireOwner);
  router.get('/queue', (_req, res) => {
    res.set('Cache-Control', 'no-store');
    try { return res.json(store.summary()); }
    catch { return res.status(503).json({ error: 'gitlab_event_store_unavailable' }); }
  });
  for (const action of ['retry', 'discard'] as const) {
    router.post(`/queue/dead-letters/:id/${action}`, mutation(`gitlab.dead_letter.${action}`), (req, res) => {
      if (!/^[a-f0-9]{64}$/.test(req.params.id)) return res.status(400).json({ error: 'invalid_dead_letter_id' });
      try {
        const result = queue.recover(req.params.id, action);
        if (result === 'not_found') return res.status(404).json({ error: 'dead_letter_not_found' });
        if (result === 'queue_full') return res.set('Retry-After', String(store.retryAfter())).status(503).json({ error: 'gitlab_event_queue_full' });
        return res.json({ ok: true });
      } catch { return res.status(503).json({ error: 'gitlab_event_store_unavailable' }); }
    });
  }
  return router;
}
