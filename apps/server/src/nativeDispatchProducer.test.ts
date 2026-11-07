import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash,randomUUID } from 'node:crypto';
import { mkdtempSync,mkdirSync,chmodSync,writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import type { NativeWorkerCatalog,ProfileSummary,RegisteredNode } from '@git-agent-harness/contracts';
import { NativeDispatchProducer } from './nativeDispatchProducer.js';
import { NativeDispatchBroker,type NativeDispatchBinding,type NativeStartGrant } from './nativeDispatchBroker.js';
import { ClaimsService } from './claimsService.js';
import type { NativeWorkerHttpTransport } from './nativeWorkerTransport.js';

function fixture() {
  const root=mkdtempSync(join(tmpdir(),'gah-native-producer-'));chmodSync(root,0o700);
  const claimsPath=join(root,'claims.json');let claims=new ClaimsService(claimsPath);
  const request={args:{profile:'test',work_id:'#1',run_id:randomUUID(),mode:'fix',backend:'auto',retries:2,dispatch_reason:'initial'}};
  let plans=0,preparations=0,stages=0,starts=0,failPreparation=false,loseReply=false;
  const workers=[{node_id:'worker-a',profiles:['test']},{node_id:'worker-b',profiles:['test']}] as RegisteredNode[];
  const transport={async catalog(scope:{node_id:string;profile:string;repo_id:string;repo:string}):Promise<NativeWorkerCatalog>{
    return {protocol:1,...scope,instances:[{backend_instance:'codex',logical_backend:'codex',enabled:true,executable_ready:true,auth_ready:null,supported_models:['fixture-model']}]};
  }} as unknown as NativeWorkerHttpTransport;
  let broker:NativeDispatchBroker;
  const deps=()=>({root,centralUrl:'https://central.invalid',coordinatorNodeId:'central',claims,
    transport:{async stage(binding:NativeDispatchBinding){stages++;return {...binding,state:'ready' as const,reservation:{...binding,id:'a'.repeat(32),expires_at:new Date(Date.now()+20000).toISOString()}};},
      async start(binding:NativeDispatchBinding,grant:NativeStartGrant){starts++;assert.equal((await broker.consumeStartGrant(grant)).allowed,true);return {...binding,state:'running' as const};},
      async observe(binding:NativeDispatchBinding){return {...binding,state:'unknown' as const};},
      async releaseReservation(binding:NativeDispatchBinding,reservation:{id:string}){return {...binding,reservation_id:reservation.id,capacity_released:true,dispatch_started:false};},
      async releaseResult(){throw Error('Fixture has no result');}},
    authorizeStart:async()=>({allowed:true as const}),authorizeContinuation:async()=>({allowed:true as const}),
    recordCanonicalResult:async()=>{throw Error('Fixture cannot import');}});
  broker=new NativeDispatchBroker(deps());
  const producer=()=>new NativeDispatchProducer({root,centralUrl:'https://central.invalid',coordinatorId:'central',claims,broker,workers:()=>workers,
    profiles:async()=>[{name:'test',repo_id:'repo',repo:'owner/repo'}] as ProfileSummary[],transport,
    plan:async request=>{plans++;return {args:{...request.args,backend:'codex',model:'fixture-model'}};},
    prepare:async options=>{
      preparations++;if(failPreparation)throw Error('PRIVATE_PREPARATION_FAILURE');
      const args=JSON.parse((await import('node:fs')).readFileSync(options.requestFile,'utf8')).args;
      mkdirSync(options.output,{mode:0o700});
      const content={'request.json':JSON.stringify({args}),'policy.json':JSON.stringify({fields:{repo_id:'repo',repo:'owner/repo'}}),'history-0000.jsonl':''};
      const hash=(bytes:string)=>createHash('sha256').update(bytes).digest('hex');
      const files=Object.fromEntries(Object.entries(content).map(([name,bytes])=>{writeFileSync(join(options.output,name),bytes,{mode:0o600});return [name,{sha256:hash(bytes),bytes:Buffer.byteLength(bytes)}];}));
      const manifest=JSON.stringify({version:2,run_id:args.run_id,node_id:options.nodeId,profile:'test',central_url:options.centralUrl,history_chunks:1,files});
      writeFileSync(join(options.output,'manifest.json'),manifest,{mode:0o600});
      const binding:NativeDispatchBinding={run_id:args.run_id,node_id:options.nodeId,context_sha256:hash(manifest),profile:'test',repo_id:'repo',repo:'owner/repo',work_id:args.work_id,
        mode:'fix',backend:'codex',model:'fixture-model',backend_instance:null,branch:null,review_generation:null,dispatch_reason:'initial'};
      writeFileSync(join(options.output,'binding.json'),JSON.stringify(binding),{mode:0o600});
      if(loseReply)throw Error('PRIVATE_LOST_PREPARATION_REPLY');return binding;
    }});
  let current=producer();
  return {root,request,workers,get producer(){return current;},get broker(){return broker;},get claims(){return claims;},
    get plans(){return plans;},get preparations(){return preparations;},get stages(){return stages;},get starts(){return starts;},
    failPreparation(){failPreparation=true;},loseReply(){loseReply=true;},
    reload(){claims=new ClaimsService(claimsPath);broker=new NativeDispatchBroker(deps());current=producer();return current;}};
}
test('producer reserves ownership without staging until explicit controller activation',async()=>{
  const f=fixture();const reserved=await f.producer.reserve(f.request);
  assert.equal(reserved.phase,'reserved');assert.equal(reserved.activated,false);assert.ok(f.claims.getLease('test','#1')?.native_handoff);
  assert.equal(f.stages,0);assert.equal(f.starts,0);
  assert.deepEqual(await f.broker.reconcilePending(),{checked:1,unconfirmed:1});assert.equal(f.stages,0);
  const replay=await f.reload().reserve({args:{...f.request.args,run_id:randomUUID(),work_id:'TICKET-001'}});
  assert.equal(replay.binding.run_id,reserved.binding.run_id);assert.equal(f.plans,1);assert.equal(f.preparations,1);
  assert.equal((await f.broker.offerPrepared(replay.binding.run_id,replay.binding.context_sha256)).phase,'running');
  assert.equal(f.stages,1);assert.equal(f.starts,1);
});
test('lost preparation reply recovers the original completed context without copying again',async()=>{
  const f=fixture();f.loseReply();await assert.rejects(f.producer.reserve(f.request));
  assert.equal(f.producer.status('test','#1')?.phase,'preparing');assert.equal(f.claims.getLease('test','#1'),undefined);
  const recovered=await f.reload().reserve({args:{...f.request.args,run_id:randomUUID()}});
  assert.equal(recovered.binding.run_id,f.request.args.run_id);assert.equal(recovered.activated,false);
  assert.equal(f.preparations,1);assert.equal(f.plans,1);assert.equal(f.stages,0);
});
test('partial preparation remains inspectable and cannot allocate another history copy',async()=>{
  const f=fixture();f.failPreparation();await assert.rejects(f.producer.reserve(f.request));
  await assert.rejects(f.reload().reserve({args:{...f.request.args,run_id:randomUUID()}}),/Original native preparation is unconfirmed/);
  assert.equal(f.producer.status('test','#1')?.run_id,f.request.args.run_id);
  assert.equal(f.preparations,1);assert.equal(f.plans,1);assert.equal(f.stages,0);assert.equal(f.starts,0);
});
test('worker selection avoids unrelated native load and never adopts an ordinary claim',async()=>{
  const f=fixture();f.claims.reserveNative('central','test','#other',{run_id:randomUUID(),node_id:'worker-a',context_sha256:'f'.repeat(64)});
  assert.equal((await f.producer.reserve(f.request)).binding.node_id,'worker-b');
  const held=fixture();held.claims.acquire('legacy-worker','test','#1');
  await assert.rejects(held.producer.reserve(held.request),/existing dispatch/);assert.equal(held.plans,0);assert.equal(held.preparations,0);
});
