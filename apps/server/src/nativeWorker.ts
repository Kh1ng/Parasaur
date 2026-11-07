/** Worker-only fixed native lifecycle. Context and executable paths are local
 * service configuration; remote requests cannot choose commands or paths. */
import { spawn,execFileSync } from 'node:child_process';
import { openSync, closeSync, realpathSync } from 'node:fs';
import { join, isAbsolute } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import { Router } from 'express';
import rateLimit from 'express-rate-limit';
import type { ProfileSummary,ConfigShowFull,NativeWorkerCatalog } from '@git-agent-harness/contracts';
import { requireOwner } from './authMiddleware.js';
import { NativeWorkerContext, nativeBinding } from './nativeWorkerContext.js';
import type { NativeDispatchBinding, NativeStartGrant, NativeWorkerObservation } from './nativeDispatchBroker.js';

const MAX_OUTPUT=32*1024*1024;
function command(binary:string,args:string[],input?:unknown):Promise<unknown> {
  const env={...process.env};
  for(const key of ['COORDINATOR_TOKEN','GAH_LEDGER_PATH','GAH_EVENTS_PATH','GAH_RECONCILIATION_PATH']) delete env[key];
  return new Promise((resolve,reject)=>{
    const child=spawn(binary,args,{env,stdio:['pipe','pipe','pipe'],windowsHide:true});
    const chunks:Buffer[]=[];let size=0;let failed=false;
    const timer=setTimeout(()=>{failed=true;child.kill('SIGKILL');},20_000);
    child.stdout.on('data',(chunk:Buffer)=>{size+=chunk.length;if(size>MAX_OUTPUT){failed=true;child.kill('SIGKILL');}else chunks.push(chunk);});
    child.stderr.resume();child.stdin.on('error',()=>undefined);
    child.once('error',()=>{clearTimeout(timer);reject(Error('Native command unavailable'));});
    child.once('close',code=>{
      clearTimeout(timer);
      if(failed || code!==0) return reject(Error('Native command unconfirmed; reconcile this run'));
      try {resolve(JSON.parse(Buffer.concat(chunks).toString()));} catch {reject(Error('Native command returned an invalid response'));}
    });
    child.stdin.end(input===undefined?undefined:JSON.stringify(input));
  });
}
export class NativeWorker {
  readonly contexts:NativeWorkerContext;
  private readonly binary:string;
  constructor(private readonly options:{root:string;nodeId:string;binary:string;config?:string;
    profiles:()=>Promise<ProfileSummary[]>;centralToken:()=>string|undefined}) {
    if(process.platform!=='linux') throw Error('Native workers require Linux');
    const executable=isAbsolute(options.binary)?options.binary:execFileSync('/usr/bin/which',[options.binary],{encoding:'utf8',timeout:2000}).trim();
    this.binary=realpathSync(executable);this.contexts=new NativeWorkerContext(options.root);
    for(const path of [this.binary,this.contexts.root,options.config??'']) if(/[%\x00-\x1f\x7f]/.test(path)) throw Error('Unsupported native service path');
  }
  async check(binding:NativeDispatchBinding):Promise<void> {
    if(!nativeBinding(binding) || binding.node_id!==this.options.nodeId) throw Error('Native worker identity changed');
    const profile=(await this.options.profiles()).find(profile=>profile.name===binding.profile);
    if(!profile || profile.repo!==binding.repo || profile.repo_id!==binding.repo_id) throw Error('Native worker profile changed');
  }
  async catalog(scope:{node_id:string;profile:string;repo_id:string;repo:string}):Promise<NativeWorkerCatalog> {
    if(scope.node_id!==this.options.nodeId || !Object.values(scope).every(value=>typeof value==='string' && value.length>0 && value.length<=4096)) throw Error('Invalid worker catalog scope');
    const profile=(await this.options.profiles()).find(profile=>profile.name===scope.profile);
    if(!profile || profile.repo!==scope.repo || profile.repo_id!==scope.repo_id) throw Error('Worker catalog repository changed');
    const configuration=await command(this.binary,['config','show','--json','--full','--profile',scope.profile,
      ...(this.options.config?['--config-path',this.options.config]:[])]) as ConfigShowFull;
    const instances=configuration.profiles[scope.profile]?.backend_instances;
    if(!instances || instances.length>128) throw Error('Worker account metadata unavailable');
    return {protocol:1,...scope,instances:instances.map(instance=>({backend_instance:instance.backend_instance,
      logical_backend:instance.logical_backend,enabled:instance.enabled===true,executable_ready:instance.executable_resolved===true,
      auth_ready:typeof instance.auth_ready==='boolean'?instance.auth_ready:null,supported_models:instance.supported_models}))};
  }
  private args(binding:NativeDispatchBinding,action:string,context=false):string[] {
    const paths=this.contexts.paths(binding);
    const args=['native-handoff',action,'--work-dir',paths.work];
    if(['worker-status','result','release-result','release-reservation'].includes(action)) args.push('--binding-file',paths.bindingFile);
    if(context) args.push('--context-dir',paths.context);
    if(['start','host-reserve'].includes(action)) args.push('--expected-context',binding.context_sha256);
    if(this.options.config && ['start','host-reserve','release-result'].includes(action)) args.push('--config-path',this.options.config);
    return args;
  }
  async stage(binding:NativeDispatchBinding):Promise<NativeWorkerObservation> {
    await this.check(binding);
    if(!this.contexts.offerStage(binding)) return this.observe(binding);
    const {run}=this.contexts.paths(binding);
    for(const name of ['admission.stdout','admission.stderr']) closeSync(openSync(join(run,name),'wx',0o600));
    const args=['--user','--quiet','--no-ask-password','--no-block','--expand-environment=no',
      `--unit=gah-native-capacity-${binding.run_id}.service`];
    for(const value of ['Type=oneshot','RemainAfterExit=yes','FileDescriptorStoreMax=1','FileDescriptorStorePreserve=yes',
      'NotifyAccess=all','Restart=no','KillMode=control-group','StopWhenUnneeded=no','UMask=0077','TimeoutStartSec=60s',
      'StandardInput=null',`StandardOutput=file:${join(run,'admission.stdout')}`,`StandardError=file:${join(run,'admission.stderr')}`,
      'UnsetEnvironment=COORDINATOR_TOKEN GAH_LEDGER_PATH GAH_EVENTS_PATH GAH_RECONCILIATION_PATH']) args.push('-p',value);
    for(const key of ['HOME','PATH','GAH_COORDINATOR_IDENTITY_PATH']) {
      const value=process.env[key];if(value===undefined) continue;
      if(/[\x00-\x1f\x7f]/.test(value)) throw Error('Invalid native lookup environment');
      args.push(`--setenv=${key}=${value}`);
    }
    // systemd-run has no JSON output. A fixed wrapper-free invocation starts
    // the holder once; observation below, not exit zero, establishes readiness.
    await new Promise<void>((resolve,reject)=>{
      const child=spawn('/usr/bin/systemd-run',[...args,this.binary,...this.args(binding,'host-reserve',true)],{stdio:'ignore'});
      const timer=setTimeout(()=>child.kill('SIGKILL'),5000);
      child.once('error',()=>{clearTimeout(timer);reject(Error('Native admission request unconfirmed'));});
      child.once('close',code=>{clearTimeout(timer);code===0?resolve():reject(Error('Native admission request unconfirmed'));});
    });
    return this.observe(binding);
  }
  async observe(binding:NativeDispatchBinding):Promise<NativeWorkerObservation> {
    const unknown:NativeWorkerObservation={...binding,state:'unknown'};
    try {
      const status=await command(this.binary,this.args(binding,'worker-status')) as {binding:NativeDispatchBinding;state:string;holder_invocation:string;capacity_held:boolean};
      if(!isDeepStrictEqual(status.binding,binding)) return unknown;
      if(status.state==='ready' && status.capacity_held===true && /^[a-f0-9]{32}$/.test(status.holder_invocation)) {
        return {...binding,state:'ready',reservation:{...binding,id:status.holder_invocation,expires_at:new Date(Date.now()+20_000).toISOString()}};
      }
      if(status.state==='running') return {...binding,state:'running'};
      const result=await command(this.binary,this.args(binding,'result',true)) as {state:string;receipt_json:string};
      if(result.state==='terminal' && typeof result.receipt_json==='string' && Buffer.byteLength(result.receipt_json)<=8*1024*1024) {
        return {...binding,state:'terminal',receipt_json:result.receipt_json};
      }
    } catch { /* Missing or busy service evidence remains unconfirmed. */ }
    return unknown;
  }
  async action(binding:NativeDispatchBinding,action:string,body:Record<string,unknown>):Promise<unknown> {
    await this.check(binding);
    if(action==='prepare') return {files:this.contexts.begin(binding),stage_offered:this.contexts.stageOffered(binding)};
    this.contexts.paths(binding);
    if(action==='stage') return this.stage(binding);
    if(action==='observe') return this.observe(binding);
    if(action==='start') {
      const grant=body.grant as NativeStartGrant|undefined;
      if(!grant || !isDeepStrictEqual([grant.run_id,grant.node_id,grant.context_sha256],[binding.run_id,binding.node_id,binding.context_sha256])
        || !/^[a-f0-9]{64}$/.test(grant.token) || !/^[a-f0-9]{32}$/.test(grant.reservation_id)
        || !Number.isFinite(Date.parse(grant.expires_at)) || Object.keys(grant).length!==6) throw Error('Invalid native start grant');
      const token=this.options.centralToken();if(!token) throw Error('Central credential unavailable');
      await command(this.binary,this.args(binding,'start',true),{grant,service_token:token});
      return this.observe(binding);
    }
    if(action==='release-reservation') {
      if(typeof body.reservation_id!=='string' || !/^[a-f0-9]{32}$/.test(body.reservation_id)) throw Error('Invalid native reservation');
      const result=await command(this.binary,[...this.args(binding,'release-reservation'),'--reservation-id',body.reservation_id]) as Record<string,unknown>;
      if(!isDeepStrictEqual(result.binding,binding)) throw Error('Native release identity changed');
      return {...binding,reservation_id:result.reservation_id,capacity_released:result.capacity_released,dispatch_started:result.dispatch_started};
    }
    if(action==='release-result') {
      const token=this.options.centralToken();if(!token) throw Error('Central credential unavailable');
      const result=await command(this.binary,this.args(binding,'release-result',true),{service_token:token}) as Record<string,unknown>;
      const spool=await command(this.binary,this.args(binding,'result',true)) as {state:string;receipt_sha256:string};
      const status=await command(this.binary,this.args(binding,'worker-status')) as {binding:NativeDispatchBinding;holder_invocation:string};
      if(![result.binding,status.binding].every(value=>isDeepStrictEqual(value,binding)) || spool.state!=='terminal'
        || !/^[a-f0-9]{64}$/.test(spool.receipt_sha256)) throw Error('Native release identity changed');
      return {...binding,reservation_id:status.holder_invocation,receipt_sha256:spool.receipt_sha256,
        capacity_released:result.capacity_released,context_history_cleaned:result.context_history_cleaned};
    }
    throw Error('Unsupported native operation');
  }
}

export function nativeWorkerRouter(worker:NativeWorker):Router {
  const router=Router();router.use(requireOwner);
  router.use(rateLimit({windowMs:60_000,limit:240,standardHeaders:true,legacyHeaders:false}));
  router.post('/catalog',async(req,res)=>{
    res.set('Cache-Control','no-store');
    const body=req.body as {node_id:string;profile:string;repo_id:string;repo:string}|undefined;
    if(!body || Object.keys(body).length!==4 || !['node_id','profile','repo_id','repo'].every(key=>Object.hasOwn(body,key))) return void res.sendStatus(400);
    try {res.json(await worker.catalog(body));}
    catch {res.status(409).json({error:'Worker account metadata unavailable'});}
  });
  router.post('/',async(req,res)=>{
    res.set('Cache-Control','no-store');
    const body=req.body as Record<string,unknown>|undefined;
    if(!body || !nativeBinding(body.binding) || typeof body.action!=='string'
      || Object.keys(body).some(key=>!['action','binding','grant','reservation_id'].includes(key))) return void res.status(400).json({error:'Invalid native worker request'});
    const binding=body.binding;
    try {res.json(await worker.contexts.exclusive(binding,()=>worker.action(binding,body.action as string,body)));}
    catch {res.status(409).json({error:'Native worker operation unconfirmed; reconcile the original run'});}
  });
  router.post('/context',async(req,res)=>{
    res.set('Cache-Control','no-store');
    if(typeof req.query.run_id!=='string' || typeof req.query.file!=='string' || req.headers['content-type']!=='application/octet-stream') return void res.sendStatus(400);
    try {
      const binding=worker.contexts.binding(req.query.run_id);
      if(req.query.node_id!==binding.node_id || req.query.context_sha256!==binding.context_sha256) return void res.sendStatus(409);
      await worker.check(binding);
      await worker.contexts.exclusive(binding,()=>worker.contexts.accept(binding,req.query.file as string,req));res.sendStatus(204);
    } catch {res.status(409).json({error:'Native context transfer unconfirmed'});}
  });
  return router;
}
