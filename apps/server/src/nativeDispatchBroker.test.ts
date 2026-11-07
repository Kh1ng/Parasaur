import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash, randomUUID } from 'node:crypto';
import { chmodSync, existsSync, mkdtempSync, mkdirSync, readFileSync, unlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import express from 'express';
import type { AddressInfo } from 'node:net';
import { nativeGrantRouter } from './nativeGrantRouter.js';
import type { NativeResultAcknowledgement } from '@git-agent-harness/contracts';
import { ClaimsService, ClaimConflictError } from './claimsService.js';
import { NativeDispatchBroker, type NativeAuthorization, type NativeDispatchBinding, type NativeHostReservation, type NativeWorkerObservation, type NativeStartGrant, type NativeOperationRequest } from './nativeDispatchBroker.js';

const hash = (value: string) => createHash('sha256').update(value).digest('hex');
function fixture() {
  let now = Date.parse('2026-11-06T19:00:00Z');
  const root = mkdtempSync(join(tmpdir(), 'gah-native-broker-')); chmodSync(root, 0o700);
  const run = randomUUID(); const runPath = join(root, run); mkdirSync(runPath, { mode: 0o700 });
  const context = join(runPath, 'context'); mkdirSync(context, { mode: 0o700 });
  const request = { args: { run_id: run, profile: 'gah', work_id: '#1525', mode: 'fix', backend: 'opencode', model: 'fixture-model' } };
  const content: Record<string,string> = { 'request.json': JSON.stringify(request), 'policy.json': JSON.stringify({ fields: { repo_id: 'fixture', repo: 'fixture/repo' } }), 'history-0000.jsonl': '' };
  const files = Object.fromEntries(Object.entries(content).map(([name, bytes]) => { writeFileSync(join(context, name), bytes, { mode: 0o600 }); return [name, { sha256: hash(bytes), bytes: Buffer.byteLength(bytes) }]; }));
  const manifest = JSON.stringify({ version: 2, node_id: 'worker', run_id: run, profile: 'gah', central_url: 'https://central.invalid', history_chunks: 1, files });
  writeFileSync(join(context, 'manifest.json'), manifest, { mode: 0o600 });
  const binding: NativeDispatchBinding = { run_id: run, node_id: 'worker', context_sha256: hash(manifest), profile: 'gah', repo_id: 'fixture', repo: 'fixture/repo', work_id: '#1525', mode: 'fix', backend: 'opencode', model: 'fixture-model', backend_instance: null, branch: null, review_generation: null, dispatch_reason: null };
  writeFileSync(join(context, 'binding.json'), JSON.stringify(binding), { mode: 0o600 });
  const claimsPath = join(root, 'claims.json'); let claims = new ClaimsService(claimsPath, () => now);
  let stages = 0; let starts = 0; let releases = 0; let imports = 0;
  const reservation = (): NativeHostReservation => ({ ...binding, id: 'reservation', expires_at: new Date(now + 20_000).toISOString() });
  let observation: NativeWorkerObservation = { ...binding, state: 'ready', reservation: reservation() };
  let authorize: () => Promise<NativeAuthorization> = async () => ({ allowed: true });
  let grant: NativeStartGrant | undefined; let consume = true;
  let stageFails = false; let startFails = false; let importFails = false; let observeFails = false; let stageHangs = false; let timeout=5000;
  const ack: NativeResultAcknowledgement = { ...binding, profile: 'gah', work_id: '#1525', receipt_sha256: 'b'.repeat(64), outcome: 'recorded' };
  const deps = () => ({ root, centralUrl: 'https://central.invalid', coordinatorNodeId: 'central', claims, now: () => now, observationTimeoutMs:timeout,
    authorizeStart: () => authorize(),
    authorizeContinuation: () => authorize(),
    transport: {
      async stage() { stages++; assert.equal(JSON.parse(readFileSync(join(runPath,'broker.json'),'utf8')).stage_offered,true); if (stageHangs) return new Promise<NativeWorkerObservation>(() => undefined); if (stageFails) throw Error('PRIVATE_STAGE_TOKEN'); return observation; },
      async start(_binding: NativeDispatchBinding, offered: NativeStartGrant) { starts++; grant = offered; assert.equal(JSON.parse(readFileSync(join(runPath,'broker.json'),'utf8')).start_offered,true); if (startFails) throw Error('PRIVATE_START_TOKEN'); if (consume && !(await broker.consumeStartGrant(offered)).allowed) throw Error('Start authority refused'); observation = { ...binding,state: 'running' }; return observation; },
      async observe() { if (observeFails) throw Error('PRIVATE_OBSERVATION_TOKEN'); return observation; },
      async releaseReservation() { releases++; },
    },
    async recordCanonicalResult({ receiptFile }: { receiptFile: string }) { imports++; if (importFails) throw Error('PRIVATE_IMPORT_TOKEN'); return { ...ack, receipt_sha256: hash(readFileSync(receiptFile,'utf8')), outcome: imports > 1 ? 'already_recorded' as const : 'recorded' as const }; },
  });
  let broker: NativeDispatchBroker = new NativeDispatchBroker(deps());
  return { root, run, runPath, context, binding, ack, request,
    get broker() { return broker; }, get claims() { return claims; }, get stages() { return stages; }, get starts() { return starts; }, get imports() { return imports; }, get releases() { return releases; },
    get grant() { assert.ok(grant); return structuredClone(grant); }, skipConsumption() { consume = false; },
    observe(value: NativeWorkerObservation) { observation = value; },
    failStage() { stageFails = true; }, failStart() { startFails = true; }, failImport() { importFails = true; }, recoverImport() { importFails = false; }, failObserve() { observeFails = true; },
    hangStage() { stageHangs=true; timeout=5; broker=new NativeDispatchBroker(deps()); },
    shortDeadline() { timeout=5; broker=new NativeDispatchBroker(deps()); },
    authorize(value: () => Promise<NativeAuthorization>) { authorize = value; }, advance() { now += 24 * 60 * 60_000; },
    reload() { claims = new ClaimsService(claimsPath, () => now); broker = new NativeDispatchBroker(deps()); return broker; },
  };
}

async function grantEndpoint(broker: NativeDispatchBroker, worker: (id: string)=>{profiles?:string[]}|undefined) {
  const app=express();app.use(express.json());
  app.use((req,res,next)=>{res.locals.authPrincipal=req.headers['x-test-principal']==='owner'?{kind:'owner'}:{kind:'device',id:'paired-browser'};next();});
  app.use('/api/native-dispatch',nativeGrantRouter({broker,worker}));
  const server=app.listen(0,'127.0.0.1');await new Promise<void>(resolve=>server.once('listening',resolve));
  const url=`http://127.0.0.1:${(server.address() as AddressInfo).port}/api/native-dispatch/consume`;
  return {
    post:(body:unknown,owner=true)=>fetch(url,{method:'POST',headers:{'content-type':'application/json','x-test-principal':owner?'owner':'device'},body:JSON.stringify(body)}),
    operation:(body:unknown,owner=true)=>fetch(url.replace('/consume','/operation'),{method:'POST',headers:{'content-type':'application/json','x-test-principal':owner?'owner':'device'},body:JSON.stringify(body)}),
    acknowledgement:(body:unknown,owner=true)=>fetch(url.replace('/consume','/acknowledgement'),{method:'POST',headers:{'content-type':'application/json','x-test-principal':owner?'owner':'device'},body:JSON.stringify(body)}),
    close:()=>new Promise<void>((resolve,reject)=>server.close(error=>error?reject(error):resolve())),
  };
}

test('concurrent exact requests persist intent before transport and stage/start once', async () => {
  const f = fixture();
  const results = await Promise.all(Array.from({ length: 6 }, () => f.broker.offerPrepared(f.run, f.binding.context_sha256)));
  assert.ok(results.every(result => result.phase === 'running'));
  assert.equal(f.stages, 1); assert.equal(f.starts, 1);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  assert.equal(readFileSync(join(f.runPath,'broker.json'),'utf8').includes(f.grant.token), false);
  assert.ok(results.every(result => result.grant_consumed_at));
  assert.equal(JSON.stringify(f.broker.get(f.run)).includes('token_sha256'),false);
});

const operation=(f:ReturnType<typeof fixture>,sequence=1,purpose:NativeOperationRequest['purpose']='attempt'):NativeOperationRequest=>({
  run_id:f.run,node_id:f.binding.node_id,context_sha256:f.binding.context_sha256,reservation_id:'reservation',sequence,purpose,
});

test('later operations require consumed start authority and exact monotonic sequences across restart',async()=>{
  const unconsumed=fixture();unconsumed.skipConsumption();await unconsumed.broker.offerPrepared(unconsumed.run,unconsumed.binding.context_sha256);
  assert.deepEqual(await unconsumed.broker.authorizeOperation(operation(unconsumed)),{allowed:false,reason:'invalid_operation'});
  const f=fixture();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  const results=await Promise.all([f.broker.authorizeOperation(operation(f)),f.broker.authorizeOperation(operation(f))]);
  assert.equal(results.filter(r=>r.allowed).length,1);
  const granted=results.find(r=>r.allowed)!;
  if(granted.allowed)assert.equal(Date.parse(granted.expires_at)-Date.parse(granted.authorized_at),10000);
  assert.deepEqual(await f.reload().authorizeOperation(operation(f)),{allowed:false,reason:'invalid_operation'});
  assert.deepEqual(await f.broker.authorizeOperation(operation(f,3)),{allowed:false,reason:'invalid_operation'});
  assert.equal((await f.broker.authorizeOperation(operation(f,2,'publish'))).allowed,true);
  f.advance(); // The original reservation's start-offer expiry is not a running job's lifetime.
  assert.equal((await f.reload().authorizeOperation(operation(f,3))).allowed,true);
  assert.equal(f.starts,1);assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('later authority is fresh and denial never consumes an operation sequence or releases ownership',async()=>{
  const f=fixture();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  for(const reason of ['factory_paused','policy_changed','budget_held','controller_changed','authority_unavailable'] as const){
    f.authorize(async()=>({allowed:false,reason}));
    assert.deepEqual(await f.broker.authorizeOperation(operation(f)),{allowed:false,reason});
    assert.equal(JSON.parse(readFileSync(join(f.runPath,'broker.json'),'utf8')).last_operation,undefined);
  }
  f.authorize(async()=>({allowed:true}));
  assert.equal((await f.broker.authorizeOperation(operation(f))).allowed,true);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('ownership changing during a later check cannot deliver permission',async()=>{
  const f=fixture();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.authorize(async()=>{
    await f.claims.completeNative('central','gah','#1525',f.binding,async()=>f.ack);
    f.claims.acquire('replacement','gah','#1525');return {allowed:true};
  });
  assert.deepEqual(await f.broker.authorizeOperation(operation(f)),{allowed:false,reason:'claim_lost'});
  assert.equal(f.claims.getLease('gah','#1525')?.node_id,'replacement');
});

test('uncertain later-permission commit never returns permission or allows replay',async t=>{
  const f=fixture();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  const rename=fs.renameSync;
  t.mock.method(fs,'renameSync',(source:fs.PathLike,destination:fs.PathLike)=>{
    rename(source,destination);if(String(destination)===join(f.runPath,'broker.json'))throw Error('Injected operation sync uncertainty');
  });syncBuiltinESMExports();
  try {assert.deepEqual(await f.broker.authorizeOperation(operation(f)),{allowed:false,reason:'authority_unavailable'});}
  finally {t.mock.restoreAll();syncBuiltinESMExports();}
  assert.deepEqual(await f.reload().authorizeOperation(operation(f)),{allowed:false,reason:'invalid_operation'});
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('terminal or forged later operations cannot obtain permission',async()=>{
  const f=fixture();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  for(const changes of [{node_id:'other'},{context_sha256:'f'.repeat(64)},{reservation_id:'other'},{sequence:0},{sequence:1.5},{purpose:'shell'}]){
    assert.equal((await f.broker.authorizeOperation({...operation(f),...changes} as NativeOperationRequest)).allowed,false);
  }
  f.observe({...f.binding,state:'terminal',receipt_json:'{"fixture":"finished"}'});
  await f.broker.reconcile(f.run);
  assert.deepEqual(await f.broker.authorizeOperation(operation(f)),{allowed:false,reason:'invalid_operation'});
});

test('later HTTP checks require service ownership, exact input and continuing profile membership',async()=>{
  const f=fixture();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  let registered=true;const endpoint=await grantEndpoint(f.broker,()=>registered?{profiles:['gah']}:undefined);
  try {
    assert.equal((await endpoint.operation(operation(f),false)).status,403);
    assert.equal((await endpoint.operation({...operation(f),command:'arbitrary'})).status,400);
    registered=false;assert.equal((await endpoint.operation(operation(f))).status,403);
    registered=true;
    f.authorize(async()=>{registered=false;return {allowed:true};});
    assert.equal((await endpoint.operation(operation(f))).status,403);
    registered=true;f.authorize(async()=>({allowed:true}));
    assert.equal((await endpoint.operation(operation(f))).status,409);
    assert.equal((await endpoint.operation(operation(f,2,'publish'))).status,200);
  } finally {await endpoint.close();}
});

test('lost start acknowledgement, expiry, restart and stale ready observations never cause another launch', async () => {
  const f = fixture(); f.failStart();
  assert.equal((await f.broker.offerPrepared(f.run, f.binding.context_sha256)).phase, 'reconciling');
  f.advance(); f.reload();
  assert.equal((await f.broker.offerPrepared(f.run, f.binding.context_sha256)).phase, 'reconciling');
  assert.equal(f.stages, 1); assert.equal(f.starts, 1);
  assert.throws(() => f.claims.acquire('another','gah','#1525'), ClaimConflictError);
  assert.equal(readFileSync(join(f.runPath,'broker.json'),'utf8').includes('PRIVATE_START_TOKEN'), false);
});

test('lost staging acknowledgement and an absent worker preserve ownership without re-offering', async () => {
  const f = fixture(); f.failStage();
  await f.broker.offerPrepared(f.run, f.binding.context_sha256);
  f.observe({ ...f.binding, state: 'unknown' }); f.reload();
  assert.equal((await f.broker.reconcile(f.run)).phase, 'reconciling');
  assert.equal(f.stages,1); assert.equal(f.starts,0);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('denied authority or invalid host reservations cannot issue a start grant', async () => {
  for (const kind of ['denied','expired','wrong-context','malformed-id'] as const) {
    const f = fixture();
    if (kind==='denied') f.authorize(async () => ({allowed:false,reason:'budget_held'}));
    else f.observe({...f.binding,state:'ready',reservation:{...f.binding,id:kind==='malformed-id'?'bad\nreservation':'reservation',context_sha256:kind==='wrong-context'?'c'.repeat(64):f.binding.context_sha256,expires_at:kind==='expired'?'2026-11-06T18:00:00Z':'2026-11-06T19:00:20Z'}});
    assert.equal((await f.broker.offerPrepared(f.run,f.binding.context_sha256)).phase,kind==='denied'?'authorization_deferred':'reconciling');
    assert.equal(f.starts,0); assert.equal(f.releases,kind==='denied'?1:0);
    assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  }
});

test('an observation deadline leaves the exact stage intent held and never repeats it', async () => {
  const f=fixture(); f.hangStage();
  assert.equal((await f.broker.offerPrepared(f.run,f.binding.context_sha256)).phase,'reconciling');
  f.observe({...f.binding,state:'unknown'}); f.reload();
  await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  assert.equal(f.stages,1); assert.equal(f.starts,0);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('a missing journal cannot recreate a held run after a lost start acknowledgement', async () => {
  const f=fixture(); f.failStart(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  unlinkSync(join(f.runPath,'broker.json')); f.reload();
  await assert.rejects(f.broker.offerPrepared(f.run,f.binding.context_sha256),/ownership exists without its broker journal/);
  assert.equal(f.stages,1); assert.equal(f.starts,1);
});

test('capacity wait does not request authority or start; a later fresh reservation can proceed', async () => {
  const f = fixture(); let authorityCalls=0;
  f.observe({ ...f.binding,state:'waiting_capacity' });
  assert.equal((await f.broker.offerPrepared(f.run,f.binding.context_sha256)).phase,'waiting_capacity');
  f.authorize(async () => { authorityCalls++; return { allowed:false,reason:'factory_paused' }; });
  assert.equal(authorityCalls,0); assert.equal(f.starts,0);
  f.observe({ ...f.binding,state:'ready',reservation:{ ...f.binding,id:'new-reservation',expires_at:'2026-11-06T19:00:20Z' } });
  await f.broker.reconcile(f.run); assert.equal(authorityCalls,1); assert.equal(f.starts,0);
});

test('canonical import failure retains spooled result and ownership until replay succeeds', async () => {
  const f = fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.failImport(); f.observe({ ...f.binding,state:'terminal',receipt_json:'{"fixture":"result"}' });
  assert.equal((await f.broker.reconcile(f.run)).phase,'result_pending');
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  f.recoverImport(); f.failObserve(); f.reload();
  assert.equal((await f.broker.reconcile(f.run)).phase,'terminal');
  assert.equal(f.claims.getLease('gah','#1525'),undefined);
  assert.equal(existsSync(join(f.context,'history-0000.jsonl')),false);
  assert.equal(existsSync(join(f.context,'binding.json')),true);
  assert.equal((await f.broker.offerPrepared(f.run,f.binding.context_sha256)).phase,'terminal');
  assert.equal(f.starts,1); assert.equal(f.stages,1);
});

test('a terminal receipt before start can reconcile preflight failure but never restart that run', async () => {
  const f = fixture(); f.failImport(); f.observe({ ...f.binding,state:'terminal',receipt_json:'{"preflight":"failed"}' });
  await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.observe({ ...f.binding,state:'ready',reservation:{ ...f.binding,id:'reservation',expires_at:'2026-11-06T19:00:20Z' } });
  await f.broker.reconcile(f.run);
  assert.equal(f.starts,0); assert.equal(f.imports,2);
});

test('changed manifest-bound controls or binding metadata cannot reuse a prepared run', async () => {
  const f = fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  writeFileSync(join(f.context,'request.json'),JSON.stringify({ args:{...f.request.args,model:'other-model'} }),{mode:0o600});
  await assert.rejects(f.broker.offerPrepared(f.run,f.binding.context_sha256), /request or policy changed/);
  assert.equal(f.starts,1); assert.equal(f.stages,1);
});

test('stale running observations cannot postpone a saved terminal result', async () => {
  const f = fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.failImport(); f.observe({ ...f.binding,state:'terminal',receipt_json:'{"fixture":"finished"}' });
  assert.equal((await f.broker.reconcile(f.run)).phase,'result_pending');
  f.recoverImport(); f.observe({ ...f.binding,state:'running' }); f.reload();
  assert.equal((await f.broker.reconcile(f.run)).phase,'terminal');
  assert.equal(f.starts,1); assert.equal(f.claims.getLease('gah','#1525'),undefined);
});

test('a mismatched canonical context acknowledgement retains ownership', async () => {
  const f = fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.ack.context_sha256 = 'c'.repeat(64);
  f.observe({ ...f.binding,state:'terminal',receipt_json:'{"fixture":"finished"}' });
  assert.equal((await f.broker.reconcile(f.run)).phase,'result_pending');
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  assert.equal(existsSync(join(f.context,'history-0000.jsonl')),true);
});

test('terminal reconciliation retries deferred history cleanup without importing or starting again', async () => {
  const f = fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  const history = join(f.context,'history-0000.jsonl');
  // A directory in place of a history file must never be removed recursively.
  unlinkSync(history); mkdirSync(history,{ mode:0o700 });
  f.observe({ ...f.binding,state:'terminal',receipt_json:'{"fixture":"finished"}' });
  const done = await f.broker.reconcile(f.run);
  assert.equal(done.phase,'terminal'); assert.equal(done.reason,'history_cleanup_pending');
  const journal = JSON.parse(readFileSync(join(f.runPath,'broker.json'),'utf8'));
  assert.equal(journal.history_cleaned,false);
  // Simulate the operator resolving the invalid path without another job run.
  const { rmdirSync } = await import('node:fs'); rmdirSync(history);
  assert.equal((await f.reload().reconcile(f.run)).reason,undefined);
  assert.equal(JSON.parse(readFileSync(join(f.runPath,'broker.json'),'utf8')).history_cleaned,true);
  assert.equal(f.imports,1); assert.equal(f.starts,1);
});

test('an uncertain committed stage intent latches the broker and reload never repeats stage', async t => {
  const f=fixture(); const rename=fs.renameSync; let writes=0;
  t.mock.method(fs,'renameSync',(source: fs.PathLike,destination: fs.PathLike) => {
    rename(source,destination);
    if (String(destination)===join(f.runPath,'broker.json') && ++writes===3) throw Error('Injected post-rename failure');
  });
  syncBuiltinESMExports();
  try {
    await assert.rejects(f.broker.offerPrepared(f.run,f.binding.context_sha256),/post-rename failure/);
    await assert.rejects(f.broker.reconcile(f.run),/persistence is uncertain/);
    assert.equal(f.stages,0); assert.equal(f.starts,0);
  } finally { t.mock.restoreAll(); syncBuiltinESMExports(); }
  f.observe({...f.binding,state:'unknown'}); f.reload();
  assert.equal((await f.broker.offerPrepared(f.run,f.binding.context_sha256)).phase,'reconciling');
  assert.equal(f.stages,0); assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('uncertain receipt persistence retains ownership and reload recovers the same result', async t => {
  const f=fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.observe({...f.binding,state:'terminal',receipt_json:'{"fixture":"finished"}'});
  const rename=fs.renameSync;
  t.mock.method(fs,'renameSync',(source: fs.PathLike,destination: fs.PathLike) => {
    rename(source,destination);
    if (/receipt-[a-f0-9]{64}\.json$/.test(String(destination))) throw Error('Injected receipt sync failure');
  });
  syncBuiltinESMExports();
  try {
    await assert.rejects(f.broker.reconcile(f.run),/persistence is uncertain/);
    assert.equal(f.imports,0); assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  } finally { t.mock.restoreAll(); syncBuiltinESMExports(); }
  assert.equal((await f.reload().reconcile(f.run)).phase,'terminal');
  assert.equal(f.imports,1); assert.equal(f.starts,1);
});

test('concurrent consumption and cold replay can authorize a grant only once', async () => {
  const f=fixture(); f.failStart(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  const grant=f.grant;
  const consumed=await Promise.all([f.broker.consumeStartGrant(grant),f.broker.consumeStartGrant(grant)]);
  assert.equal(consumed.filter(result=>result.allowed).length,1);
  assert.ok(f.broker.get(f.run).grant_consumed_at);
  assert.deepEqual(await f.reload().consumeStartGrant(grant),{allowed:false,reason:'already_consumed'});
  await f.broker.reconcile(f.run);
  assert.equal(f.starts,1); assert.equal(f.stages,1);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  assert.equal(readFileSync(join(f.runPath,'broker.json'),'utf8').includes(grant.token),false);
});

test('forged identity, reservation, expiry or token cannot consume a grant, and expired grants stay held', async () => {
  const f=fixture(); f.failStart(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  const grant=f.grant;
  for(const changes of [{node_id:'other'},{context_sha256:'b'.repeat(64)},{reservation_id:'other'},{expires_at:'2026-11-06T19:00:11Z'},{token:'0'.repeat(64)}]) {
    assert.deepEqual(await f.broker.consumeStartGrant({...grant,...changes}),{allowed:false,reason:'invalid_grant'});
  }
  assert.equal(f.broker.get(f.run).grant_consumed_at,undefined);
  f.advance(); f.reload();
  assert.deepEqual(await f.broker.consumeStartGrant(grant),{allowed:false,reason:'expired'});
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('authority revoked between offer and consumption cannot authorize execution', async () => {
  const f=fixture(); f.failStart(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.authorize(async()=>({allowed:false,reason:'factory_paused'}));
  assert.deepEqual(await f.broker.consumeStartGrant(f.grant),{allowed:false,reason:'factory_paused'});
  assert.equal(f.broker.get(f.run).grant_consumed_at,undefined);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('a grant expiring during the fresh check cannot be consumed or release its ownership',async()=>{
  const f=fixture();f.failStart();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.authorize(async()=>{f.advance();return {allowed:true};});
  assert.deepEqual(await f.broker.consumeStartGrant(f.grant),{allowed:false,reason:'expired'});
  assert.equal(f.broker.get(f.run).grant_consumed_at,undefined);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('an authority timeout during consumption neither burns permission nor keeps an in-memory consumer', async () => {
  const f=fixture(); f.failStart(); await f.broker.offerPrepared(f.run,f.binding.context_sha256); f.shortDeadline();
  f.authorize(()=>new Promise(()=>undefined));
  assert.deepEqual(await f.broker.consumeStartGrant(f.grant),{allowed:false,reason:'authority_unavailable'});
  assert.equal(f.broker.get(f.run).grant_consumed_at,undefined);
  f.authorize(async()=>({allowed:true}));
  assert.equal((await f.broker.consumeStartGrant(f.grant)).allowed,true);
});

test('claim replacement during asynchronous authority checks refuses consumption and preserves the new owner', async () => {
  const f=fixture(); f.failStart(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.authorize(async()=>{
    await f.claims.completeNative('central','gah','#1525',f.binding,async()=>f.ack);
    f.claims.acquire('new-owner','gah','#1525');
    return {allowed:true};
  });
  assert.deepEqual(await f.broker.consumeStartGrant(f.grant),{allowed:false,reason:'claim_lost'});
  assert.equal(f.broker.get(f.run).grant_consumed_at,undefined);
  assert.equal(f.claims.getLease('gah','#1525')?.node_id,'new-owner');
});

test('an uncertain consumed-grant commit never returns permission and cold reload refuses replay', async t => {
  const f=fixture(); f.failStart(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  const grant=f.grant,rename=fs.renameSync;
  t.mock.method(fs,'renameSync',(source: fs.PathLike,destination: fs.PathLike)=>{
    rename(source,destination);
    if(String(destination)===join(f.runPath,'broker.json'))throw Error('Injected consumed grant sync failure');
  });syncBuiltinESMExports();
  try {
    assert.deepEqual(await f.broker.consumeStartGrant(grant),{allowed:false,reason:'authority_unavailable'});
    assert.throws(()=>f.broker.get(f.run),/persistence is uncertain/);
  } finally {t.mock.restoreAll();syncBuiltinESMExports();}
  assert.deepEqual(await f.reload().consumeStartGrant(grant),{allowed:false,reason:'already_consumed'});
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('a worker running observation without durable grant consumption remains unconfirmed', async () => {
  const f=fixture();f.skipConsumption();
  const result=await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  assert.equal(result.phase,'reconciling');assert.equal(result.reason,'execution_without_start_authority');
  assert.equal(result.grant_consumed_at,undefined);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('HTTP consumption requires owner service auth and current registered profile membership', async()=>{
  const f=fixture();f.failStart();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  let registered=true,declared=true;
  const endpoint=await grantEndpoint(f.broker,id=>registered && id==='worker'?{profiles:declared?['gah']:[]}:undefined);
  const {post}=endpoint;
  try {
    const grant=f.grant;
    assert.equal((await post(grant,false)).status,403);
    assert.equal((await post({...grant,contextDirectory:'/untrusted'})).status,400);
    registered=false;assert.equal((await post(grant)).status,403);
    registered=true;declared=false;assert.equal((await post(grant)).status,403);
    assert.equal(f.broker.get(f.run).grant_consumed_at,undefined);
    declared=true;
    const consumed=await post(grant);assert.equal(consumed.status,200);
    const receipt=await consumed.json() as Record<string,unknown>;assert.equal(receipt.allowed,true);assert.equal(receipt.token,undefined);
    const replay=await post(grant);assert.equal(replay.status,409);assert.equal((await replay.json() as Record<string,unknown>).reason,'already_consumed');
    assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  } finally {await endpoint.close();}
});

test('registry revocation during a grant check never delivers permission or allows replay',async()=>{
  const f=fixture();f.failStart();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  let registered=true;
  const endpoint=await grantEndpoint(f.broker,()=>registered?{profiles:['gah']}:undefined);
  try {
    f.authorize(async()=>{registered=false;return {allowed:true};});
    assert.equal((await endpoint.post(f.grant)).status,403);
    assert.ok(f.broker.get(f.run).grant_consumed_at);
    registered=true;assert.equal((await endpoint.post(f.grant)).status,409);
    assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  } finally {await endpoint.close();}
});

test('release acknowledgement requires exact durable canonical import and survives expiry/restart/pause',async()=>{
  const f=fixture();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  const receipt='{"fixture":"terminal"}';
  const request={run_id:f.run,node_id:f.binding.node_id,context_sha256:f.binding.context_sha256,
    reservation_id:f.grant.reservation_id,receipt_sha256:hash(receipt)};
  assert.deepEqual(f.broker.acknowledgeResult(request),{allowed:false,reason:'result_pending'});
  f.failImport();f.observe({...f.binding,state:'terminal',receipt_json:receipt});
  assert.equal((await f.broker.reconcile(f.run)).phase,'result_pending');
  assert.deepEqual(f.broker.acknowledgeResult(request),{allowed:false,reason:'result_pending'});
  f.recoverImport();assert.equal((await f.reload().reconcile(f.run)).phase,'terminal');
  const expected=f.broker.acknowledgeResult(request);assert.equal(expected.allowed,true);
  f.advance();f.authorize(async()=>({allowed:false,reason:'factory_paused'}));
  assert.deepEqual(f.reload().acknowledgeResult(request),expected);
  for(const key of ['node_id','context_sha256','reservation_id','receipt_sha256'] as const){
    assert.deepEqual(f.broker.acknowledgeResult({...request,[key]:key.endsWith('sha256')?'f'.repeat(64):'wrong'}),
      {allowed:false,reason:'invalid_acknowledgement'});
  }
  assert.equal(f.starts,1);assert.equal(f.releases,0);assert.equal(f.imports,2);
});

test('acknowledgement endpoint is owner-only, strictly scoped and checks current worker membership',async()=>{
  const f=fixture();await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  const receipt='{"fixture":"terminal"}';
  const request={run_id:f.run,node_id:f.binding.node_id,context_sha256:f.binding.context_sha256,
    reservation_id:f.grant.reservation_id,receipt_sha256:hash(receipt)};
  let registered=true;
  const endpoint=await grantEndpoint(f.broker,()=>({profiles:registered?['gah']:[]}));
  try{
    assert.equal((await endpoint.acknowledgement(request,false)).status,403);
    assert.equal((await endpoint.acknowledgement({...request,command:'release'})).status,400);
    assert.equal((await endpoint.acknowledgement(request)).status,409);
    f.observe({...f.binding,state:'terminal',receipt_json:receipt});await f.broker.reconcile(f.run);
    const response=await endpoint.acknowledgement(request);assert.equal(response.status,200);
    const result=await response.json() as {allowed:boolean;acknowledgement:NativeResultAcknowledgement};
    assert.equal(result.allowed,true);assert.equal(result.acknowledgement.receipt_sha256,hash(receipt));
    registered=false;assert.equal((await endpoint.acknowledgement(request)).status,403);
    registered=true;assert.equal((await endpoint.acknowledgement({...request,receipt_sha256:'f'.repeat(64)})).status,409);
  }finally{await endpoint.close();}
});
