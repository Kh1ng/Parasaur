import { Router } from 'express';
import { loadGitLabRunsFeed, parseGitLabRunsQuery } from './runsFeed.js';

/**
 * Read-only runs feed for the GitLab panel (#1595). Mounted under
 * /api/integrations/gitlab AFTER the shared /api authMiddleware: unlike the
 * hook-secret events route (#1592), this answers the same authenticated
 * callers as the rest of the API, including the embed view's
 * same-origin/browser fetches.
 */
export function gitLabRunsRouter(): Router {
  const router = Router();
  router.get('/runs', async (req, res) => {
    const context = parseGitLabRunsQuery(req.query as Record<string, unknown>);
    if ('error' in context) {
      res.status(400).json({ error: 'invalid_gitlab_context', message: context.error });
      return;
    }
    try {
      res.json(await loadGitLabRunsFeed(context));
    } catch (error) {
      res.status(502).json({
        error: 'Failed to load GitLab runs feed',
        message: error instanceof Error ? error.message : String(error)
      });
    }
  });
  return router;
}
