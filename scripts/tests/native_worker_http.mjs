/** Opt-in Linux/systemd integration, driven by native_host_reservation.py.
 * Real broker, HTTP, context store, worker service and CLI. Either admission
 * is denied or the worker loses the reply to its one-time start request. */
import assert from 'node:assert/strict';
import { readFileSync,existsSync,writeFileSync } from 'node:fs';
import { randomUUID } from 'node:crypto';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const [runtime,binary,fixture,listenerFd,mode]=process.argv.slice(2);
const loseConsumption = listenerFd !== undefined;
const modules=pathToFileURL(`${runtime.replace(/\/$/,'')}/`);
const [{NativeWorker,nativeWorkerRouter},{NativeWorkerHttpTransport},{NativeDispatchBroker},{ClaimsService},{authMiddleware},{nativeGrantRouter}]=await Promise.all([
  import(new URL('nativeWorker.js',modules)),import(new URL('nativeWorkerTransport.js',modules)),
  import(new URL('nativeDispatchBroker.js',modules)),import(new URL('claimsService.js',modules)),import(new URL('authMiddleware.js',modules)),import(new URL('nativeGrantRouter.js',modules)),
]);
const express=(await import(new URL('../../../node_modules/express/index.js',modules))).default;
const binding=JSON.parse(readFileSync(join(fixture,'binding.json'),'utf8'));
const root=join(fixture,'runs'),claimsPath=join(fixture,'http-claims.json');
process.env.COORDINATOR_TOKEN='native-http-fixture-service';
const worker=new NativeWorker({root:join(fixture,'remote'),nodeId:binding.node_id,binary,config:join(fixture,'worker.toml'),
  profiles:async()=>[{name:binding.profile,repo:binding.repo,repo_id:binding.repo_id}],centralToken:()=>process.env.COORDINATOR_TOKEN});
const app=express();app.use(express.json());app.use(authMiddleware);
let releaseRequests=0, consumedReplies=0, operationRequests=0;
app.use((req,res,next)=>{
  if(loseConsumption && req.path==='/api/native-dispatch/consume') {
    // Drop only the successful HTTP reply after the real broker writes its journal.
    res.end=(()=>{if(res.statusCode===200) consumedReplies++;req.socket.destroy();return res;});
  }
  if(req.path==='/api/native-dispatch/operation') operationRequests++;
  if(req.body?.action==='release-reservation' && ++releaseRequests===1) {
    res.end=(()=>{req.socket.destroy();return res;});
  }
  next();
});
app.use('/api/native-worker',nativeWorkerRouter(worker));
const server=loseConsumption ? app.listen({fd:Number(listenerFd)}) : app.listen(0,'127.0.0.1');
await new Promise(resolve=>server.once('listening',resolve));
const node={node_id:binding.node_id,advertised_url:`http://127.0.0.1:${server.address().port}`,
  transport_mode:'authenticated_remote',secret_ref:'env:COORDINATOR_TOKEN',profiles:[binding.profile]};
let claims=new ClaimsService(claimsPath),broker;
const dependencies=()=>({root,centralUrl:loseConsumption ? node.advertised_url : 'https://native-host-fixture.invalid',coordinatorNodeId:'fixture-central',claims,
  transport:new NativeWorkerHttpTransport({contextRoot:root,worker:id=>id===binding.node_id?node:undefined}),
  authorizeStart:async()=>loseConsumption ? {allowed:true} : {allowed:false,reason:'factory_paused'},
  authorizeContinuation:async()=>{throw Error('No native attempt is authorized');},
  recordCanonicalResult:async()=>{throw Error('Unused reservation cannot invent a result');},
});
const execute=promisify(execFile);
async function collectExitedStart() {
  const job=`gah-native-job-${binding.run_id}.service`, holder=`gah-native-capacity-${binding.run_id}.service`;
  const ctl=async(...args)=>(await execute('/usr/bin/systemctl',['--user',...args],{timeout:3000})).stdout.trim();
  const deadline=Date.now()+5000;
  while(await ctl('show',job,'-p','ActiveState','--value')!=='failed') {
    assert.ok(Date.now()<deadline,'Original launcher did not exit');
    await new Promise(resolve=>setTimeout(resolve,50));
  }
  const original=await ctl('show',job,'-p','InvocationID','--value');
  assert.match(original,/^[a-f0-9]{32}$/);
  assert.equal(await ctl('show',job,'-p','MainPID','--value'),'0');
  assert.equal(await ctl('show',job,'-p','ExecMainStatus','--value'),'1');
  await ctl('reset-failed',job);
  assert.equal((await worker.observe(binding)).state,'unknown','Collection of only the launcher leaves the reservation held');
  await ctl('stop',holder);
  for(const unit of [job,holder]) assert.equal(await ctl('show',unit,'-p','LoadState','--value'),'not-found');
  // The two missing units and an empty cgroup are insufficient on their own.
  // A different launch in this disposable journal must fail both observations.
  const work=worker.contexts.paths(binding).work, path=join(work,'worker.json');
  const bytes=readFileSync(path), journal=JSON.parse(bytes);
  assert.equal(journal.launch.invocation,original);
  journal.launch.invocation=randomUUID().replaceAll('-','');
  try {
    writeFileSync(path,JSON.stringify(journal));
    assert.equal((await worker.observe(binding)).state,'unknown');
    await assert.rejects(worker.action(binding,'release-reservation',{reservation_id:journal.holder_invocation}));
    assert.equal(existsSync(join(work,'reservation-release.json')),false);
  } finally {
    writeFileSync(path,bytes);
  }
  const observation=await worker.observe(binding);
  assert.equal(observation.state,'exited_before_dispatch','Worker lost the archived launcher exit after collection');
  assert.equal(observation.exited_job_invocation,original);
}
try {
  broker=new NativeDispatchBroker(dependencies());
  app.use('/api/native-dispatch',nativeGrantRouter({broker,worker:id=>id===binding.node_id?node:undefined}));
  await broker.offerPrepared(binding.run_id,binding.context_sha256);
  const deadline=Date.now()+25_000;
  let collected=false;
  while(releaseRequests===0) {
    const view=broker.get(binding.run_id);
    assert.ok(Date.now()<deadline,`Native HTTP admission still ${view.phase}: ${view.reason ?? ''}`);
    await new Promise(resolve=>setTimeout(resolve,250));
    if(mode==='http-collected-start' && consumedReplies && !collected) {
      assert.ok(claims.getLease(binding.profile,binding.work_id)?.native_handoff);
      await collectExitedStart();
      collected=true;
    }
    await broker.reconcile(binding.run_id);
  }
  const pending=broker.get(binding.run_id);
  assert.equal(collected,mode==='http-collected-start');
  assert.equal(pending.phase,'cancelling');
  assert.ok(claims.getLease(binding.profile,binding.work_id)?.native_handoff);
  claims=new ClaimsService(claimsPath);broker=new NativeDispatchBroker(dependencies());
  const retired=await broker.reconcile(binding.run_id);
  assert.equal(retired.phase,'cancelled');
  assert.equal(claims.getLease(binding.profile,binding.work_id),undefined);assert.equal(releaseRequests,2);
  const paths=worker.contexts.paths(binding);
  assert.equal(JSON.parse(readFileSync(join(paths.work,'reservation-release.json'),'utf8')).phase,'released');
  const journal=JSON.parse(readFileSync(join(paths.work,'worker.json'),'utf8'));
  if(loseConsumption) {
    assert.equal(consumedReplies,1);
    assert.ok(pending.grant_consumed_at);
    assert.equal(journal.consumption_offered,true);
    assert.match(journal.launch.invocation,/^[a-f0-9]{32}$/);
    assert.equal(retired.reservation_release.proof.exited_job_invocation,journal.launch.invocation);
    assert.equal(journal.execution,null);
    assert.ok(!journal.operation && !journal.claim_operation);
  } else {
    assert.equal(journal.start,null);
  }
  assert.equal(operationRequests,0);
  assert.equal(existsSync(join(paths.work,'result.json')),false);
  const lease=JSON.parse(readFileSync(join(paths.work,'capacity.json'),'utf8'));
  const proof=await execute('/usr/bin/python3',['-c','import fcntl,sys\nwith open(sys.argv[1]) as f:fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)\nprint("free")',lease.path],{timeout:5000});
  assert.equal(proof.stdout.trim(),'free');
  assert.equal((await broker.offerPrepared(binding.run_id,binding.context_sha256)).phase,'cancelled');
  assert.equal(releaseRequests,2);
  console.log(JSON.stringify({passed:true,actualBroker:true,actualAuthenticatedHttp:true,actualWorkerService:true,
    actualContextTransfer:true,actualDefaultAdmission:mode!=='http-collected-start',collected,actualKernelRelease:true,lostReleaseReplyRecovered:true,
    centralClaimRetainedUntilProof:true,centralReloadNoDuplicateStart:true,backendInvoked:false,consumedReplies,operationRequests,releaseRequests}));
} finally {
  server.closeAllConnections();await new Promise(resolve=>server.close(resolve));
  for(const unit of [`gah-native-job-${binding.run_id}.service`,`gah-native-capacity-${binding.run_id}.service`]) {
    await execute('/usr/bin/systemctl',['--user','stop',unit],{timeout:8000}).catch(()=>undefined);
    await execute('/usr/bin/systemctl',['--user','reset-failed',unit],{timeout:3000}).catch(()=>undefined);
  }
}
