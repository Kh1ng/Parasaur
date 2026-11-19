import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash,randomUUID } from 'node:crypto';
import { mkdtempSync,mkdirSync,writeFileSync,chmodSync,readFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import type { AddressInfo } from 'node:net';
import express from 'express';
import type { RegisteredNode } from '@git-agent-harness/contracts';
import { NativeWorkerContext } from './nativeWorkerContext.js';
import { nativeWorkerRouter,type NativeWorker } from './nativeWorker.js';
import { NativeWorkerHttpTransport } from './nativeWorkerTransport.js';
import type { NativeDispatchBinding,NativeStartGrant } from './nativeDispatchBroker.js';

async function fixture() {
  const root=mkdtempSync(join(tmpdir(),'gah-native-http-'));chmodSync(root,0o700);
  const source=join(root,'source'),destination=join(root,'worker');mkdirSync(source,{mode:0o700});mkdirSync(destination,{mode:0o700});
  const run=randomUUID(),runPath=join(source,run),context=join(runPath,'context');mkdirSync(runPath,{mode:0o700});mkdirSync(context,{mode:0o700});
  const contents={'request.json':'{"fixture":"request"}','policy.json':'{"fixture":"policy"}','history-0000.jsonl':'{"fixture":"history"}\n'};
  const hash=(bytes:string)=>createHash('sha256').update(bytes).digest('hex');
  const manifest=JSON.stringify({version:2,run_id:run,node_id:'worker',profile:'fixture',history_chunks:1,
    files:Object.fromEntries(Object.entries(contents).map(([name,bytes])=>[name,{bytes:Buffer.byteLength(bytes),sha256:hash(bytes)}]))});
  for(const [name,bytes] of Object.entries({...contents,'manifest.json':manifest})) writeFileSync(join(context,name),bytes,{mode:0o600});
  const binding:NativeDispatchBinding={run_id:run,node_id:'worker',context_sha256:hash(manifest),profile:'fixture',repo:'fixture/repo',repo_id:'fixture',work_id:'#1',mode:'fix',backend:'codex',model:'fixture-model',backend_instance:null,branch:null,review_generation:null,dispatch_reason:'initial'};
  const contexts=new NativeWorkerContext(destination);let stages=0,starts=0,loseUpload=false,member=true,corruptCatalog=false;
  const check=async(candidate:NativeDispatchBinding)=>{if(!member || candidate.node_id!=='worker' || candidate.repo!==binding.repo) throw Error('Profile changed');};
  const ready=()=>({...binding,state:'ready',reservation:{...binding,id:'a'.repeat(32),expires_at:new Date(Date.now()+20_000).toISOString()}});
  const worker={contexts,check,async catalog(scope:Record<string,string>){
    if(!member)throw Error('Profile changed');
    return {protocol:1,...scope,instances:[{backend_instance:'codex',logical_backend:'codex',enabled:true,
      executable_ready:true,auth_ready:null,supported_models:['fixture-model'],...(corruptCatalog?{private_token:'PRIVATE_UNEXPECTED_CATALOG_FIELD'}:{})}]};
  },async action(candidate:NativeDispatchBinding,action:string){
    await check(candidate);
    if(action==='prepare') return {files:contexts.begin(candidate),stage_offered:contexts.stageOffered(candidate)};
    if(action==='stage') {if(contexts.offerStage(candidate)) stages++;return ready();}
    if(action==='observe') return ready();
    if(action==='start') {starts++;return {...binding,state:'running'};}
    if(action==='release-reservation') return {...binding,reservation_id:'a'.repeat(32),capacity_released:true,dispatch_started:false};
    if(action==='release-result') return {...binding,reservation_id:'a'.repeat(32),receipt_sha256:'b'.repeat(64),capacity_released:true,context_history_cleaned:true};
    throw Error('Unknown fixture operation');
  }} as unknown as NativeWorker;
  const app=express();app.use(express.json());
  app.use((req,res,next)=>{
    res.locals.authPrincipal={kind:req.headers.authorization==='Bearer fixture-service'?'owner':'device'};
    if(loseUpload && req.path.endsWith('/context')) {
      loseUpload=false;
      res.end=(()=>{req.socket.destroy();return res;}) as typeof res.end;
    }
    next();
  });
  app.use('/api/native-worker',nativeWorkerRouter(worker));
  const server=app.listen(0,'127.0.0.1');await new Promise<void>(resolve=>server.once('listening',resolve));
  const url=`http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  process.env.NATIVE_TRANSPORT_TEST_CREDENTIAL='fixture-service';
  const node={node_id:'worker',advertised_url:url,transport_mode:'authenticated_remote',secret_ref:'env:NATIVE_TRANSPORT_TEST_CREDENTIAL',profiles:['fixture']} as RegisteredNode;
  const transport=new NativeWorkerHttpTransport({contextRoot:source,worker:id=>id==='worker'?node:undefined});
  return {binding,context,contexts,contents,source,node,url,transport,
    get stages(){return stages;},get starts(){return starts;},loseUpload(){loseUpload=true;},revoke(){member=false;},
    corruptCatalog(){corruptCatalog=true;},
    close:()=>new Promise<void>((resolve,reject)=>server.close(error=>error?reject(error):resolve()))};
}
test('real HTTP transfers immutable files and carries fixed lifecycle observations and proofs',async()=>{
  const f=await fixture();
  try {
    assert.equal((await f.transport.stage(f.binding,f.context)).state,'ready');
    assert.equal((await f.transport.stage(f.binding,f.context)).state,'ready');assert.equal(f.stages,1);
    for(const [name,bytes] of Object.entries(f.contents)) assert.equal(readFileSync(join(f.contexts.paths(f.binding).context,name),'utf8'),bytes);
    assert.equal((await f.transport.observe(f.binding)).state,'ready');
    const grant:NativeStartGrant={...f.binding,reservation_id:'a'.repeat(32),expires_at:new Date(Date.now()+10_000).toISOString(),token:'c'.repeat(64)};
    assert.equal((await f.transport.start(f.binding,grant)).state,'running');assert.equal(f.starts,1);
    const reservation={...f.binding,id:'a'.repeat(32),expires_at:grant.expires_at};
    assert.equal((await f.transport.releaseReservation(f.binding,reservation)).dispatch_started,false);
    assert.equal((await f.transport.releaseResult(f.binding,reservation)).receipt_sha256,'b'.repeat(64));
  } finally {await f.close();}
});
test('lost upload replies resume transfer without replacing committed files or repeating admission',async()=>{
  const f=await fixture();
  try {
    f.loseUpload();await assert.rejects(f.transport.stage(f.binding,f.context),/unconfirmed/);
    assert.equal((await f.transport.observe(f.binding)).state,'unknown');
    assert.equal((await f.transport.stage(f.binding,f.context)).state,'ready');
    assert.equal(f.stages,1);assert.equal(f.starts,0);
  } finally {await f.close();}
});
test('worker HTTP requires owner service auth, exact local identity and registered membership',async()=>{
  const f=await fixture();
  try {
    const response=await fetch(`${f.url}/api/native-worker`,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({action:'prepare',binding:f.binding})});
    assert.equal(response.status,403);
    process.env.NATIVE_TRANSPORT_TEST_WRONG='PRIVATE_WRONG_TOKEN';
    const bad=new NativeWorkerHttpTransport({contextRoot:f.source,worker:()=>({...f.node,secret_ref:'env:NATIVE_TRANSPORT_TEST_WRONG'})});
    await assert.rejects(bad.stage(f.binding,f.context),error=>!String(error).includes('PRIVATE_WRONG_TOKEN'));
    await assert.rejects(f.transport.stage(f.binding,join(f.context,'escape')),/outside/);
    f.node.profiles=[];await assert.rejects(f.transport.observe(f.binding),/membership/);
    f.node.profiles=['fixture'];f.revoke();await assert.rejects(f.transport.stage(f.binding,f.context),/unconfirmed/);
    assert.equal(f.stages,0);assert.equal(f.starts,0);
  } finally {await f.close();}
});

test('catalog is owner-scoped, preserves unknown authentication and refuses extra credential fields',async()=>{
  const f=await fixture();const scope={node_id:f.binding.node_id,profile:f.binding.profile,repo:f.binding.repo,repo_id:f.binding.repo_id};
  try {
    const response=await fetch(`${f.url}/api/native-worker/catalog`,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(scope)});
    assert.equal(response.status,403);
    const catalog=await f.transport.catalog(scope);assert.equal(catalog.instances[0].auth_ready,null);
    assert.deepEqual(await f.transport.catalog(f.binding),catalog);
    assert.equal(f.stages,0);assert.equal(f.starts,0);
    f.corruptCatalog();await assert.rejects(f.transport.catalog(scope),error=>String(error).includes('Invalid worker account metadata') && !String(error).includes('PRIVATE_'));
  }finally{await f.close();}
});
