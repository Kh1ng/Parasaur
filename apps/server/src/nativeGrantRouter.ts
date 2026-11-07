import { Router,type RequestHandler } from 'express';
import rateLimit from 'express-rate-limit';
import { requireOwner } from './authMiddleware.js';
import { isNativeRunId, type NativeDispatchBroker, type NativeStartGrant, type NativeOperationRequest, type NativeAcknowledgementRequest } from './nativeDispatchBroker.js';
import type { NativeDispatchProducer } from './nativeDispatchProducer.js';
import type { NativeJobRequest } from './gahCli.js';

/** Internal worker exchange only. A paired browser or GitLab identity cannot
 * consume the service credential. No paths, dispatch commands or token issue
 * operation are accepted here. The host must still fence its own reservation. */
export function nativeGrantRouter(deps: {
  broker: NativeDispatchBroker;
  producer?:NativeDispatchProducer;
  worker: (id: string) => { profiles?: string[] } | undefined;
}): Router {
  const router = Router();
  router.use(requireOwner);
  router.use(rateLimit({windowMs:60_000,limit:60,standardHeaders:true,legacyHeaders:false}));
  router.post('/reserve',async(req,res)=>{
    if(!deps.producer) return void res.status(503).json({error:'Native controller preparation unavailable'});
    try {res.set('Cache-Control','no-store').json(await deps.producer.reserve(req.body as NativeJobRequest));}
    catch {res.status(409).json({error:'Native preparation unconfirmed; inspect its original work/run'});}
  });
  const holds:RequestHandler=(req,res)=>{
    const profile=req.method==='POST'?req.body?.profile:req.query.profile;
    if(typeof profile!=='string' || !profile || profile.length>128) return void res.sendStatus(400);
    try {res.set('Cache-Control','no-store').json(deps.broker.heldWork(profile));}
    catch {res.status(503).json({error:'Native ownership unavailable'});}
  };
  router.get('/holds',holds);router.post('/holds',holds);
  router.post('/pending-activation',(req,res)=>{
    const profile=req.body?.profile;
    if(typeof profile!=='string' || !profile || profile.length>128 || Object.keys(req.body).length!==1) return void res.sendStatus(400);
    try {res.set('Cache-Control','no-store').json(deps.broker.pendingActivation(profile));}
    catch {res.status(503).json({error:'Native dormant reservations unavailable'});}
  });
  router.get('/preparation',(req,res)=>{
    if(!deps.producer || typeof req.query.profile!=='string' || typeof req.query.work_id!=='string'
      || req.query.profile.length>128 || req.query.work_id.length>4096) return void res.sendStatus(400);
    try {const state=deps.producer.status(req.query.profile,req.query.work_id);
      res.set('Cache-Control','no-store').status(state?200:404).json(state??{error:'No native preparation'});
    }catch {res.status(503).json({error:'Native preparation observation unavailable'});}
  });
  router.post('/offer',async(req,res)=>{
    const request=req.body as {run_id?:unknown;context_sha256?:unknown}|undefined;
    if(!request || Object.keys(request).length!==2 || !isNativeRunId(request.run_id)
      || typeof request.context_sha256!=='string' || !/^[a-f0-9]{64}$/.test(request.context_sha256)) {
      return void res.status(400).json({error:'Invalid prepared native offer'});
    }
    try {
      res.json(await deps.broker.offerPrepared(request.run_id,request.context_sha256));
    } catch {res.status(409).json({error:'Native offer unconfirmed; inspect the original run'});}
  });
  router.get('/runs/:run', (req,res)=>{
    if(!isNativeRunId(req.params.run)) return void res.sendStatus(400);
    try {res.set('Cache-Control','no-store').json(deps.broker.get(req.params.run));}
    catch {res.status(404).json({error:'Native run unavailable'});}
  });
  const membership=(request:{run_id:string;node_id:string})=>{
    const binding=deps.broker.get(request.run_id).binding;
    return ()=>binding.node_id===request.node_id && !!deps.worker(request.node_id)?.profiles?.includes(binding.profile);
  };
  router.post('/acknowledgement',(req,res)=>{
    const request=req.body as NativeAcknowledgementRequest | undefined;
    const fields=['run_id','node_id','context_sha256','reservation_id','receipt_sha256'] as const;
    if (!request || Array.isArray(request) || typeof request!=='object' || Object.keys(request).length!==fields.length
      || !fields.every(key=>Object.hasOwn(request,key) && typeof request[key]==='string')
      || !isNativeRunId(request.run_id) || request.node_id.length>128
      || !/^[a-f0-9]{64}$/.test(request.context_sha256) || !/^[a-f0-9]{64}$/.test(request.receipt_sha256)
      || !request.reservation_id || request.reservation_id.length>4096) {
      return void res.status(400).json({allowed:false,reason:'invalid_acknowledgement'});
    }
    try {
      const member=membership(request);
      if (!member()) return void res.status(403).json({allowed:false,reason:'worker_membership_unavailable'});
      const result=deps.broker.acknowledgeResult(request);
      if (result.allowed && !member()) return void res.status(403).json({allowed:false,reason:'worker_membership_unavailable'});
      res.status(result.allowed?200:result.reason==='authority_unavailable'?503:409).json(result);
    } catch {res.status(503).json({allowed:false,reason:'authority_unavailable'});}
  });
  router.post('/operation',async(req,res)=>{
    const request=req.body as NativeOperationRequest | undefined;
    const fields=['run_id','node_id','context_sha256','reservation_id','sequence','purpose'] as const;
    if (!request || Array.isArray(request) || typeof request!=='object' || Object.keys(request).length!==fields.length
      || !fields.every(key=>Object.hasOwn(request,key)) || !isNativeRunId(request.run_id)
      || typeof request.node_id!=='string' || request.node_id.length>128
      || typeof request.context_sha256!=='string' || !/^[a-f0-9]{64}$/.test(request.context_sha256)
      || typeof request.reservation_id!=='string' || !request.reservation_id || request.reservation_id.length>4096
      || !Number.isSafeInteger(request.sequence) || request.sequence<1 || !['attempt','publish'].includes(request.purpose)) {
      return void res.status(400).json({allowed:false,reason:'invalid_operation'});
    }
    try {
      const member=membership(request);
      if (!member()) return void res.status(403).json({allowed:false,reason:'worker_membership_unavailable'});
      const result=await deps.broker.authorizeOperation(request);
      if (result.allowed && !member()) return void res.status(403).json({allowed:false,reason:'worker_membership_unavailable'});
      res.status(result.allowed?200:result.reason==='authority_unavailable'?503:409).json(result);
    } catch {res.status(503).json({allowed:false,reason:'authority_unavailable'});}
  });
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
      const member=membership(grant);
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
