/** Controller-facing preparation and ownership transfer. A durable per-work
 * index recovers the original preparation after a lost reply; it never creates
 * another full history copy just because the controller supplied another UUID. */
import { createHash } from 'node:crypto';
import { existsSync,mkdirSync,lstatSync,readFileSync,closeSync } from 'node:fs';
import { join } from 'node:path';
import type { RegisteredNode,ProfileSummary } from '@git-agent-harness/contracts';
import { canonicalWorkId,type ClaimsService } from './claimsService.js';
import { NativeDispatchBroker,isNativeRunId,type NativeDispatchView,type NativeDispatchBinding } from './nativeDispatchBroker.js';
import { NativeWorkerHttpTransport } from './nativeWorkerTransport.js';
import { nativeBinding,openNativeFile } from './nativeWorkerContext.js';
import { writeFileAtomic } from './atomicFile.js';
import { runPlanNativeJob,runPrepareNativeJob,type NativeJobRequest } from './gahCli.js';

interface Preparation {version:1;profile:string;work_id:string;run_id:string;node_id:string;phase:'preparing'|'prepared'}
const read=<T>(path:string):T=>{const fd=openNativeFile(path);try{return JSON.parse(readFileSync(fd).toString()) as T;}finally{closeSync(fd);}};
function directory(path:string):void {
  if(!existsSync(path)) mkdirSync(path,{mode:0o700});
  const info=lstatSync(path);
  if(!info.isDirectory() || info.isSymbolicLink() || (process.platform!=='win32' && (info.uid!==process.getuid!() || (info.mode&0o077)!==0))) throw Error('Native preparation directory is not private');
}
function identity(request:NativeJobRequest):{profile:string;work:string;run:string} {
  const args=request?.args;
  if(!request || Object.keys(request).length!==1 || !args || typeof args!=='object' || Array.isArray(args)
    || typeof args.profile!=='string' || !args.profile || args.profile.length>128 || typeof args.work_id!=='string'
    || !args.work_id || args.work_id.length>4096 || !isNativeRunId(args.run_id)) throw Error('Invalid native controller request');
  return {profile:args.profile,work:args.work_id,run:args.run_id};
}

export class NativeDispatchProducer {
  private active?:{key:string;promise:Promise<NativeDispatchView>};
  private uncertain=false;
  private readonly index:string;
  constructor(private readonly deps:{root:string;centralUrl:string;coordinatorId:string;broker:NativeDispatchBroker;claims:ClaimsService;
    workers:()=>RegisteredNode[];profiles:()=>Promise<ProfileSummary[]>;transport:NativeWorkerHttpTransport;
    plan?:typeof runPlanNativeJob;prepare?:typeof runPrepareNativeJob}) {
    this.index=join(deps.root,'preparations');directory(this.index);
  }
  private path(profile:string,work:string):string {
    return join(this.index,`${createHash('sha256').update(JSON.stringify([profile,canonicalWorkId(work)])).digest('hex')}.json`);
  }
  private save(path:string,value:unknown):void {
    if(this.uncertain) throw Error('Native preparation persistence unconfirmed; reload its index');
    try {writeFileAtomic(path,JSON.stringify(value),0o600,process.platform!=='win32');}
    catch(error) {this.uncertain=true;throw error;}
  }
  reserve(request:NativeJobRequest):Promise<NativeDispatchView> {
    const scope=identity(request);const key=this.path(scope.profile,scope.work);
    if(this.uncertain) return Promise.reject(Error('Native preparation persistence unconfirmed'));
    if(this.active) return this.active.key===key?this.active.promise:Promise.reject(Error('Another native preparation is in progress'));
    const promise=this.prepare(request);this.active={key,promise};
    void promise.finally(()=>{if(this.active?.promise===promise)this.active=undefined;}).catch(()=>undefined);
    return promise;
  }
  status(profile:string,work:string):Preparation|undefined {
    const path=this.path(profile,work);
    return existsSync(path)?this.readIndex(path,profile,work):undefined;
  }
  private readIndex(path:string,profile:string,work:string):Preparation {
    const state=read<Preparation>(path);
    if(!state || Object.keys(state).length!==6 || state.version!==1 || state.profile!==profile
      || typeof state.work_id!=='string' || canonicalWorkId(state.work_id)!==canonicalWorkId(work)
      || !isNativeRunId(state.run_id) || typeof state.node_id!=='string' || !/^[a-zA-Z0-9_-]{1,128}$/.test(state.node_id)
      || !['preparing','prepared'].includes(state.phase)) throw Error('Native preparation index changed');
    return state;
  }
  private async prepare(request:NativeJobRequest):Promise<NativeDispatchView> {
    const scope=identity(request);const path=this.path(scope.profile,scope.work);
    const held=this.deps.claims.getLease(scope.profile,scope.work);
    if(held) {
      if(!held.native_handoff) throw Error('This work is held by an existing dispatch');
      const view=this.deps.broker.get(held.native_handoff.run_id);
      if(view.binding.profile!==scope.profile || canonicalWorkId(view.binding.work_id)!==canonicalWorkId(scope.work)) throw Error('Native work binding unavailable');
      return view;
    }
    if(existsSync(path)) {
      const previous=this.readIndex(path,scope.profile,scope.work);
      const run=join(this.deps.root,previous.run_id);directory(run);
      const bindingPath=join(run,'context','binding.json');
      let old:NativeDispatchView|undefined;
      if(existsSync(join(run,'broker.json'))) old=this.deps.broker.get(previous.run_id);
      if(!old || !['terminal','cancelled'].includes(old.phase) || previous.run_id===scope.run) {
        if(!existsSync(bindingPath)) throw Error('Original native preparation is unconfirmed; inspect its run');
        const binding=read<NativeDispatchBinding>(bindingPath);
        if(!nativeBinding(binding) || binding.run_id!==previous.run_id || binding.node_id!==previous.node_id
          || binding.profile!==scope.profile || canonicalWorkId(binding.work_id)!==canonicalWorkId(scope.work)) throw Error('Prepared native binding changed');
        const reserved=await this.deps.broker.reservePrepared(binding.run_id,binding.context_sha256);
        this.save(path,{...previous,phase:'prepared'});return reserved;
      }
    }
    const profile=(await this.deps.profiles()).find(profile=>profile.name===scope.profile);
    if(!profile) throw Error('Native project configuration unavailable');
    const holds=this.deps.claims.getLeases();
    const load=(id:string)=>holds.filter(lease=>lease.native_handoff?.node_id===id).length;
    const workers=this.deps.workers().filter(worker=>worker.node_id!==this.deps.coordinatorId && worker.profiles?.includes(scope.profile))
      .sort((left,right)=>load(left.node_id)-load(right.node_id) || left.node_id.localeCompare(right.node_id));
    if(workers.length===0) throw Error(`No registered worker declares profile '${scope.profile}'`);
    let selected:{node:RegisteredNode;request:NativeJobRequest}|undefined;
    const refused:string[]=[];
    for(const node of workers) {
      try {
        const catalog=await this.deps.transport.catalog({node_id:node.node_id,profile:scope.profile,repo_id:profile.repo_id,repo:profile.repo},AbortSignal.timeout(5000));
        const planned=await (this.deps.plan??runPlanNativeJob)(request,catalog);
        if(identity(planned).run!==scope.run || planned.args.profile!==scope.profile || planned.args.work_id!==scope.work) throw Error('Native route planner changed work identity');
        selected={node,request:planned};break;
      } catch (error) {
        // Only a compatible worker with a canonical route may receive context.
        refused.push(`${node.display_name||node.node_id}:${error instanceof Error?error.message:String(error)}`);
      }
    }
    if(!selected) throw Error(`No compatible worker has an eligible native route (${refused.join('; ')})`);
    const preparation:Preparation={version:1,profile:scope.profile,work_id:scope.work,run_id:scope.run,node_id:selected.node.node_id,phase:'preparing'};
    const run=join(this.deps.root,scope.run);directory(run);
    // Index intent precedes the potentially large context copy. A partial
    // preparation is inspected/recovered, never silently replaced or rerun.
    this.save(path,preparation);
    const requestFile=join(run,'producer-request.json');this.save(requestFile,selected.request);
    const binding=await (this.deps.prepare??runPrepareNativeJob)({requestFile,output:join(run,'context'),nodeId:selected.node.node_id,centralUrl:this.deps.centralUrl});
    if(!nativeBinding(binding) || binding.run_id!==scope.run || binding.node_id!==selected.node.node_id || binding.profile!==scope.profile
      || binding.work_id!==scope.work) throw Error('Native preparation returned another binding');
    const result=await this.deps.broker.reservePrepared(binding.run_id,binding.context_sha256);
    this.save(path,{...preparation,phase:'prepared'});return result;
  }
}
