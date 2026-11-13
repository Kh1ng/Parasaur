import { Router } from 'express';
import { ClaimConflictError, type ClaimsService } from './claimsService.js';

/** Central participates under its own identity and locally configured profiles;
 * remote execution nodes participate only under their registry declarations.
 * Profile discovery failure must never grant a local lease. Store mutations
 * remain synchronous after authorization, preserving claim arbitration. */
export function claimsRouter(deps: {
  claims: () => ClaimsService;
  localNodeId: string;
  profiles: () => Promise<Array<{ name: string }>>;
  node: (id: string) => { profiles?: string[] } | undefined;
}) {
  const router = Router();
  for (const action of ['acquire', 'renew', 'release'] as const) {
    router.post(`/${action}`, async (req, res) => {
      const { node_id: nodeId, profile, work_id: workId } = req.body ?? {};
      if (typeof nodeId !== 'string' || !nodeId || typeof profile !== 'string' || !profile || typeof workId !== 'string' || !workId) {
        return void res.status(400).json({ error: 'Bad Request', message: 'A node_id, profile and work_id are required.' });
      }
      if (nodeId === deps.localNodeId) {
        let profiles;
        try {
          profiles = await deps.profiles();
          if (!Array.isArray(profiles) || !profiles.every(item => item && typeof item.name === 'string')) throw new Error('Invalid profile discovery');
        }
        catch {
          return void res.status(503).json({ error: 'Profile membership unavailable', message: 'Cannot verify the central node\'s configured profiles. Retry after profile discovery recovers.' });
        }
        if (!profiles.some(item => item.name === profile)) {
          return void res.status(400).json({ error: 'Bad Request', message: `Central has no configured profile '${profile}'.` });
        }
      } else {
        const node = deps.node(nodeId);
        if (!node) return void res.status(400).json({ error: 'Bad Request', message: `Node '${nodeId}' is not registered` });
        if (!node.profiles?.includes(profile)) {
          return void res.status(400).json({ error: 'Bad Request', message: `Node '${nodeId}' has not declared profile '${profile}' at registration` });
        }
      }
      let claims;
      try { claims = deps.claims(); }
      catch (error) {
        console.error(`Claims store failed to load: ${error instanceof Error ? error.message : String(error)}`);
        return void res.status(500).json({ error: 'Claims store unavailable', message: 'The claims store could not be loaded; check server logs.' });
      }
      try {
        if (action === 'release') {
          claims.release(nodeId, profile, workId);
          return void res.json({ success: true });
        }
        return void res.json(claims[action](nodeId, profile, workId, req.body.lease_seconds));
      } catch (error) {
        if (error instanceof ClaimConflictError) {
          return void res.status(409).json({ error: 'Conflict', message: error.message, held_by: error.heldBy });
        }
        return void res.status(400).json({ error: 'Bad Request', message: error instanceof Error ? error.message : String(error) });
      }
    });
  }
  return router;
}
