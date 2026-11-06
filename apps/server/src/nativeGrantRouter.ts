import { Router } from 'express';
import rateLimit from 'express-rate-limit';
import { requireOwner } from './authMiddleware.js';
import { isNativeRunId, type NativeDispatchBroker, type NativeStartGrant } from './nativeDispatchBroker.js';

/** Internal worker exchange only. A paired browser or GitLab identity cannot
 * consume the service credential. No paths, dispatch commands or token issue
 * operation are accepted here. The host must still fence its own reservation. */
export function nativeGrantRouter(deps: {
  broker: NativeDispatchBroker;
  worker: (id: string) => { profiles?: string[] } | undefined;
}): Router {
  const router = Router();
  router.use(requireOwner);
  router.use(rateLimit({windowMs:60_000,limit:60,standardHeaders:true,legacyHeaders:false}));
  router.post('/consume',async(req,res)=>{
    const grant=req.body as NativeStartGrant | undefined;
    const fields=['run_id','node_id','context_sha256','reservation_id','expires_at','token'] as const;
    if (!grant || Array.isArray(grant) || typeof grant !== 'object' || Object.keys(grant).length !== fields.length
      || !fields.every(key=>Object.hasOwn(grant,key) && typeof grant[key]==='string') || !isNativeRunId(grant.run_id)
      || grant.node_id.length>128 || grant.context_sha256.length!==64 || grant.token.length!==64
      || grant.reservation_id.length>4096 || grant.expires_at.length>64) {
      return void res.status(400).json({allowed:false,reason:'invalid_grant'});
    }
    try {
      const binding=deps.broker.get(grant.run_id).binding;
      const member=()=>binding.node_id===grant.node_id && !!deps.worker(grant.node_id)?.profiles?.includes(binding.profile);
      if (!member()) {
        return void res.status(403).json({error:'Worker membership unavailable',message:'The reserved worker must still declare this profile.'});
      }
      const result=await deps.broker.consumeStartGrant(grant);
      // Registry changes during the fresh CLI check can burn this grant, but
      // must never deliver permission to a worker that was just revoked.
      if (result.allowed && !member()) return void res.status(403).json({error:'Worker membership unavailable',message:'Worker access changed during authorization.'});
      res.status(result.allowed?200:result.reason==='authority_unavailable'?503:409).json(result);
    } catch { res.status(503).json({allowed:false,reason:'authority_unavailable'}); }
  });
  return router;
}
