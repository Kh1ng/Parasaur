/** Opt-in Linux/systemd acceptance: real controller -> producer -> HTTP worker.
 * Uses disposable Git/forge/backend fixtures, default physical admission and
 * the actual fresh central authority. No installed service or real issue changes.
 * Usage: node scripts/tests/native_controller_http.mjs <compiled API dist> <gah>
 */
import assert from 'node:assert/strict';
import { mkdtempSync,mkdirSync,writeFileSync,readFileSync,existsSync,readdirSync,chmodSync } from 'node:fs';
import { join,resolve,dirname } from 'node:path';
import { homedir } from 'node:os';
import { randomBytes } from 'node:crypto';
import { spawnSync,execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { pathToFileURL } from 'node:url';

assert.equal(process.platform,'linux');
const [runtime,binaryArgument,mode='normal']=process.argv.slice(2);
assert.ok(['normal','lost-reservation','lost-activation'].includes(mode));
const binary=resolve(binaryArgument),modules=pathToFileURL(`${resolve(runtime)}/`);
const execute=promisify(execFile);
const [{NativeWorker,nativeWorkerRouter},{NativeWorkerHttpTransport},{NativeDispatchBroker},
  {NativeDispatchProducer},{ClaimsService},{authMiddleware},{nativeGrantRouter},
  {nativeDispatchAuthority,nativeContinuationAuthority},{runReceiveNativeResult}]=await Promise.all([
  'nativeWorker','nativeWorkerTransport','nativeDispatchBroker','nativeDispatchProducer',
  'claimsService','authMiddleware','nativeGrantRouter','nativeDispatchAuthority','gahCli'
].map(name=>import(new URL(`${name}.js`,modules))));
const express=(await import(new URL('../../../node_modules/express/index.js',modules))).default;
const base=mkdtempSync(join(homedir(),'.cache/parasaur-controller-http.'));chmodSync(base,0o700);
const home=join(base,'home'),bins=join(base,'bin'),repo=join(base,'repo'),state=join(base,'state'),root=join(base,'runs');
for(const path of [home,bins,repo,state,root])mkdirSync(path,{mode:0o700});
const owner=randomBytes(32).toString('hex'),workerId='controller-http-fixture';
for(const key of Object.keys(process.env))if(key.startsWith('GAH_'))delete process.env[key];
Object.assign(process.env,{HOME:home,PATH:`${bins}:/usr/bin:/bin`,GITHUB_TOKEN:'fixture-token',COORDINATOR_TOKEN:owner,
  GAH_BINARY:binary,GAH_LEDGER_PATH:join(state,'ledger.jsonl'),GAH_EVENTS_PATH:join(state,'events.jsonl'),
  GAH_CLAIM_STATE_PATH:join(base,'local-claims.json'),GAH_COORDINATOR_IDENTITY_PATH:join(base,'identity.json'),
  XDG_CONFIG_HOME:join(home,'.config'),XDG_DATA_HOME:join(home,'.local/share'),XDG_STATE_HOME:join(home,'.local/state')});
writeFileSync(process.env.GAH_COORDINATOR_IDENTITY_PATH,JSON.stringify({node_id:workerId}),{mode:0o600});
function sync(command,args) {
  const result=spawnSync(command,args,{env:process.env,encoding:'utf8',timeout:30_000});
  assert.equal(result.status,0,`${command}: ${result.stderr}`);return result.stdout;
}
sync('git',['init','-b','main',repo]);
sync('git',['-C',repo,'config','user.name','Native Fixture']);
sync('git',['-C',repo,'config','user.email','fixture@invalid']);
writeFileSync(join(repo,'README.md'),'Disposable controller fixture\n');
sync('git',['-C',repo,'add','README.md']);sync('git',['-C',repo,'-c','commit.gpgSign=false','commit','-m','fixture']);
const origin=join(base,'github','fixture','diagnostic.git');mkdirSync(join(base,'github','fixture'),{recursive:true,mode:0o700});
sync('git',['init','--bare',origin]);sync('git',['-C',repo,'remote','add','origin',origin]);sync('git',['-C',repo,'push','-u','origin','main']);
sync('git',['config','--file',join(home,'.gitconfig'),`url.file://${join(base,'github')}/.insteadOf`,'https://github.com/']);
const marker=join(base,'backend-invocations'),backend=join(bins,'codex');
writeFileSync(backend,`#!/bin/sh
if [ "$1" = login ]; then echo 'Logged in using ChatGPT'; exit 0; fi
if [ "$1" != exec ]; then echo 'fixture codex'; exit 0; fi
printf 'attempt\\n' >> '${marker}'
printf 'worker edit\\n' >> README.md
printf '{"type":"turn.completed","usage":{"input_tokens":11,"output_tokens":7}}\\n'
`,{mode:0o700});
// Only the authority's managed-loop observation is controlled. Worker lifecycle
// code uses absolute systemctl/systemd-run and real kernel admission throughout.
writeFileSync(join(bins,'systemctl'),'#!/bin/sh\nprintf "LoadState=loaded\\nActiveState=active\\nMainPID=123\\nActiveEnterTimestamp=fixture-controller\\n"\n',{mode:0o700});
const forge=join(base,'forge');mkdirSync(forge,{mode:0o700});
for(const name of ['main.py','model.py','api.py'])writeFileSync(join(forge,name),readFileSync(new URL(`../../tests/support/forge/${name}`,import.meta.url)),{mode:0o600});
const forgeState=join(forge,'state.json');
writeFileSync(forgeState,JSON.stringify({now:new Date().toISOString(),login:'fixture',repo:'fixture/diagnostic',project:'42',
  issues:{},pulls:{},branches:{},checks:{},pipelines:{},labels:[],next_number:1,next_id:1,calls:[],effects:[]}),{mode:0o600});
writeFileSync(join(bins,'gh'),`#!/bin/sh\nexec /usr/bin/python3 '${join(forge,'main.py')}' '${forgeState}' '${join(forge,'commands.log')}' gh "$@"\n`,{mode:0o700});
sync('gh',['api','repos/fixture/diagnostic/issues','--method','POST','--raw-field','title=Disposable native controller task',
  '--raw-field','body=Update README.md. Acceptance criteria: README.md contains worker edit.', '--raw-field','labels=exec:autonomous']);
const profile={name:'fixture',repo_id:'controller-http',repo:'fixture/diagnostic'};
const config=join(base,'central.toml'),workerConfig=join(base,'worker.toml');
Object.assign(process.env,{GAH_CONFIG:config,GAH_CONFIG_PATH:config});
const app=express();app.use(express.json({limit:'9mb'}));app.use(authMiddleware);
let dropReservationReply=mode==='lost-reservation';
let dropActivationReply=mode==='lost-activation';
app.use((req,res,next)=>{
  if(dropReservationReply && req.path==='/api/native-dispatch/reserve') {
    dropReservationReply=false;res.end=(()=>{req.socket.destroy();return res;});
  }
  if(dropActivationReply && req.path==='/api/native-dispatch/offer') {
    dropActivationReply=false;res.end=(()=>{req.socket.destroy();return res;});
  }
  next();
});
const api=app.listen(0,'127.0.0.1');await new Promise(done=>api.once('listening',done));
const centralUrl=`http://127.0.0.1:${api.address().port}`;
function configuration(role) {
  return `[defaults]
node_role = "${role}"
registry_central_url = ${JSON.stringify(centralUrl)}
artifact_root = ${JSON.stringify(state)}
worktree_base = ${JSON.stringify(join(base,'worktrees'))}
[profiles.fixture]
display_name = "Disposable controller HTTP fixture"
repo_id = "${profile.repo_id}"
repo = "${profile.repo}"
provider = "github"
local_path = ${JSON.stringify(repo)}
artifact_root = ${JSON.stringify(state)}
default_target_branch = "main"
codex_path = ${JSON.stringify(role==='worker'?backend:join(base,'absent-central-codex'))}
validation_commands = ["true"]
[profiles.fixture.publishing]
allow_pull_request_creation = false
[profiles.fixture.routing]
improve_candidates = [{backend="codex",instance="work",model="fixture-model"}]
[profiles.fixture.routing.backend_instances.work]
runner_kind = "codex"
logical_backend = "codex"
account_label = "work"
executable = ${JSON.stringify(role==='worker'?backend:join(base,'absent-central-codex'))}
supported_models = ["fixture-model"]
`;
}
writeFileSync(config,configuration('central'),{mode:0o600});writeFileSync(workerConfig,configuration('worker'),{mode:0o600});
const worker=new NativeWorker({root:join(base,'remote'),nodeId:workerId,binary,config:workerConfig,profiles:async()=>[profile],centralToken:()=>owner});
const remote=express();remote.use(express.json({limit:'9mb'}));remote.use(authMiddleware);remote.use('/api/native-worker',nativeWorkerRouter(worker));
const remoteApi=remote.listen(0,'127.0.0.1');await new Promise(done=>remoteApi.once('listening',done));
const node={node_id:workerId,advertised_url:`http://127.0.0.1:${remoteApi.address().port}`,transport_mode:'authenticated_remote',secret_ref:'fixture',profiles:['fixture']};
const transport=new NativeWorkerHttpTransport({contextRoot:root,worker:id=>id===workerId?node:undefined,secret:()=>owner});
let claims,broker,producer;
function reload() {
  claims=new ClaimsService(join(base,'claims.json'));
  const holds=profile=>broker.heldWork(profile);
  broker=new NativeDispatchBroker({root,centralUrl,coordinatorNodeId:'fixture-central',claims,transport,
    authorizeStart:nativeDispatchAuthority(root,centralUrl,config,(binding,signal)=>transport.catalog(binding,signal),holds),
    authorizeContinuation:nativeContinuationAuthority(root,centralUrl,config,(binding,signal)=>transport.catalog(binding,signal),holds),
    recordCanonicalResult:files=>runReceiveNativeResult({...files,config})});
  producer=new NativeDispatchProducer({root,centralUrl,coordinatorId:'fixture-central',claims,broker,transport,workers:()=>[node],profiles:async()=>[profile]});
}
reload();
app.use('/api/native-dispatch',nativeGrantRouter({get broker(){return broker;},get producer(){return producer;},worker:id=>id===workerId?node:undefined}));
app.get('/api/skills/resolve',(_req,res)=>res.json({profile:'fixture',backend:'codex',instance:'work',source:'fixture',skills:[]}));
app.post('/api/events',(_req,res)=>res.sendStatus(204));
let binding;
try {
  const loop=await execute(binary,['loop','--profile','fixture','--config-path',config,'--once'],{env:process.env,timeout:45_000,maxBuffer:128*1024});
  writeFileSync(join(base,'controller.stdout'),loop.stdout,{mode:0o600});
  writeFileSync(join(base,'controller.stderr'),loop.stderr,{mode:0o600});
  const prepared=producer.status('fixture','#1');
  assert.ok(prepared,`Controller did not prepare work: ${loop.stdout} ${loop.stderr}`);
  binding=broker.get(prepared.run_id).binding;
  assert.equal(binding.backend_instance,'work');assert.equal(binding.model,'fixture-model');
  assert.equal(binding.work_id,'#1');
  if(mode==='lost-reservation') {
    assert.equal(broker.get(binding.run_id).activated,false);assert.equal(existsSync(marker),false);
    reload();
    const retry=await execute(binary,['loop','--profile','fixture','--config-path',config,'--once'],{env:process.env,timeout:45_000,maxBuffer:128*1024});
    writeFileSync(join(base,'controller-retry.stdout'),retry.stdout,{mode:0o600});
    writeFileSync(join(base,'controller-retry.stderr'),retry.stderr,{mode:0o600});
  }
  assert.equal(broker.get(binding.run_id).activated,true,'A dormant reservation must resume after the controller loses its acknowledgement');
  if(mode==='lost-activation') {
    assert.ok(loop.stdout.includes('activation_unconfirmed'));reload();
  }
  const deadline=Date.now()+60_000;
  let view=broker.get(binding.run_id);
  while(view.phase!=='terminal' || view.worker_release?.phase!=='released') {
    assert.notEqual(view.phase,'cancelled',`Native authority refused execution: ${view.reason}; fixture ${base}`);
    assert.ok(Date.now()<deadline,`Native dispatch did not finish: ${JSON.stringify(view)}; fixture ${base}`);
    await new Promise(done=>setTimeout(done,200));view=await broker.reconcile(binding.run_id);
  }
  assert.equal(claims.getLease('fixture','#1'),undefined);
  assert.equal(view.worker_release.context_history_cleaned,true);
  assert.equal(readFileSync(marker,'utf8'),'attempt\n');
  const rows=readFileSync(process.env.GAH_LEDGER_PATH,'utf8').trim().split('\n').map(JSON.parse);
  const result=rows.filter(row=>row.session_id===binding.run_id && row.mode==='fix');
  assert.equal(result.length,1);assert.equal(result[0].attempts_started,1);
  assert.equal(result[0].validation_result,'passed');
  assert.equal(result[0].remote_execution.node_id,workerId);
  assert.equal(result[0].remote_execution.receipt_sha256,view.acknowledgement.receipt_sha256);
  assert.equal(result[0].usage.actual_cost_usd,null);
  const lease=JSON.parse(readFileSync(join(worker.contexts.paths(binding).work,'capacity.json'),'utf8'));
  await execute('/usr/bin/python3',['-c','import fcntl,sys\nwith open(sys.argv[1]) as file: fcntl.flock(file,fcntl.LOCK_EX|fcntl.LOCK_NB)',lease.path],{timeout:5000});
  const before=readFileSync(process.env.GAH_LEDGER_PATH,'utf8');reload();
  await broker.reconcilePending();assert.equal(broker.get(binding.run_id).phase,'terminal');
  assert.equal(readFileSync(process.env.GAH_LEDGER_PATH,'utf8'),before);
  assert.equal(readFileSync(marker,'utf8'),'attempt\n');
  assert.equal(readdirSync(root).filter(name=>name!== 'preparations').length,1);
  const report={passed:true,fixtureRoot:base,actualControllerCycle:true,actualRustPlanning:true,centralExecutableAbsent:true,
    actualWorkerAccountCatalog:true,actualAuthenticatedHttp:true,actualFreshCentralAuthority:true,controlledManagedLoopObservation:true,
    actualDefaultSystemdAdmission:true,actualBackendFixtureAttempts:1,actualCanonicalImport:true,actualAcknowledgedRelease:true,
    reloadDoesNotRelaunchOrReimport:true,realModelInvoked:false,productionServicesChanged:false,runId:binding.run_id,
    outcome:result[0].validation_result,actualCostUsd:result[0].usage.actual_cost_usd,mode,
    lostReservationRecoveredAcrossCentralReload:mode==='lost-reservation',
    lostActivationRecoveredAcrossCentralReload:mode==='lost-activation'};
  const release=JSON.parse(readFileSync(new URL('../../../../release.json',modules),'utf8'));
  report.apiCoreRevision=release.core;report.apiPrivateRevision=release.enterprise;
  const manifest=JSON.parse(readFileSync(join(dirname(binary),'manifest.json'),'utf8'));
  report.binarySourceRevision=manifest.sourceRevision;report.binarySha256=manifest.binarySha256;
  writeFileSync(join(base,'report.json'),JSON.stringify(report,null,2)+'\n',{mode:0o600});console.log(JSON.stringify(report));
} catch(error) {
  process.stderr.write(`Native controller acceptance failed; inspect ${base}\n`);throw error;
} finally {
  for(const server of [api,remoteApi]){server.closeAllConnections();await new Promise(done=>server.close(done));}
  const run=binding?.run_id??producer.status('fixture','#1')?.run_id;
  if(run)for(const kind of ['job','capacity'])await execute('/usr/bin/systemctl',['--user','stop',`gah-native-${kind}-${run}.service`],{timeout:8000}).catch(()=>undefined);
}
