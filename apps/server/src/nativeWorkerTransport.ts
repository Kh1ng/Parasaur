/** Central native delivery over the existing registered worker HTTP endpoint.
 * Context transfer can resume. Start is sent once by the broker; its unknown
 * outcome is observed, never replayed by this transport. */
import http from 'node:http';
import https from 'node:https';
import { createReadStream, readFileSync, closeSync, fstatSync } from 'node:fs';
import { join, resolve } from 'node:path';
import type { Readable } from 'node:stream';
import type { RegisteredNode, NativeReservationRelease,NativeWorkerCatalog } from '@git-agent-harness/contracts';
import type { NativeDispatchBinding, NativeHostReservation, NativeStartGrant, NativeWorkerObservation, NativeWorkerRelease, NativeWorkerTransport } from './nativeDispatchBroker.js';
import { nativeContextManifest, openNativeFile } from './nativeWorkerContext.js';

export class NativeWorkerHttpTransport implements NativeWorkerTransport {
  private readonly transfers=new Map<string,Promise<NativeWorkerObservation>>();
  private readonly root:string;
  constructor(private readonly options:{contextRoot:string;worker:(id:string)=>RegisteredNode|undefined;secret:(reference:string)=>string}) {
    this.root=resolve(options.contextRoot);
  }
  private target(binding:Pick<NativeDispatchBinding,'node_id'|'profile'>) {
    const node=this.options.worker(binding.node_id);
    if(!node?.profiles?.includes(binding.profile)) throw Error('Native worker membership unavailable');
    const url=new URL(node.advertised_url);
    if(!['http:','https:'].includes(url.protocol) || url.username || url.password || url.pathname!=='/' || url.search || url.hash) throw Error('Invalid native worker origin');
    const token=node.secret_ref?this.options.secret(node.secret_ref):'';
    if((node.transport_mode==='authenticated_remote' && !token) || /[\x00-\x1f\x7f]/.test(token)) throw Error('Native worker credential unavailable');
    return {origin:url.origin,reference:node.secret_ref,token};
  }
  private async request(binding:Pick<NativeDispatchBinding,'node_id'|'profile'>,path:string,payload:Record<string,unknown>|Readable,signal?:AbortSignal):Promise<unknown> {
    const target=this.target(binding);const url=new URL(path,target.origin);
    const streaming=typeof (payload as Readable).pipe==='function';
    const source=streaming?payload as Readable:undefined;
    const result=await new Promise<unknown>((resolve,reject)=>{
      const request=(url.protocol==='https:'?https:http).request(url,{method:'POST',signal,headers:{
        ...(target.token?{authorization:`Bearer ${target.token}`} : {}),
        'content-type':streaming?'application/octet-stream':'application/json',
      }},response=>{
        let size=0;const chunks:Buffer[]=[];
        response.on('data',(chunk:Buffer)=>{size+=chunk.length;if(size>32*1024*1024) request.destroy(Error('Native reply exceeds its bound'));else chunks.push(chunk);});
        response.once('error',()=>reject(Error('Native worker reply unconfirmed')));
        response.once('end',()=>{
          if(response.statusCode!==200 && response.statusCode!==204) return reject(Error('Native worker operation unconfirmed'));
          try {resolve(response.statusCode===204?undefined:JSON.parse(Buffer.concat(chunks).toString()));}
          catch {reject(Error('Invalid native worker reply'));}
        });
      });
      request.setTimeout(120_000,()=>request.destroy(Error('Native worker deadline')));
      request.once('error',()=>{source?.destroy();reject(Error('Native worker exchange unconfirmed'));});
      request.once('close',()=>source?.destroy());
      if(source) {source.once('error',()=>request.destroy(Error('Native context input unavailable')));source.pipe(request);}
      else request.end(JSON.stringify(payload));
    });
    const current=this.target(binding);
    if(current.origin!==target.origin || current.reference!==target.reference || current.token!==target.token) throw Error('Native worker access changed during delivery');
    return result;
  }
  private action(binding:NativeDispatchBinding,action:string,extra:Record<string,unknown>={},signal?:AbortSignal) {
    return this.request(binding,'/api/native-worker',{action,binding,...extra},signal);
  }
  async catalog(scope:Pick<NativeDispatchBinding,'node_id'|'profile'|'repo_id'|'repo'>,signal?:AbortSignal):Promise<NativeWorkerCatalog> {
    // Authority callers hold a complete binding. Pick the wire fields explicitly:
    // TypeScript's structural Pick does not remove extra runtime properties.
    const identity={node_id:scope.node_id,profile:scope.profile,repo_id:scope.repo_id,repo:scope.repo};
    const catalog=await this.request(identity,'/api/native-worker/catalog',identity,signal) as NativeWorkerCatalog;
    const bounded=(value:unknown):value is string=>typeof value==='string' && value.length>0 && value.length<=4096 && !/[\x00-\x1f\x7f]/.test(value);
    const ids=new Set<string>();
    if(!catalog || catalog.protocol!==1 || catalog.node_id!==scope.node_id || catalog.profile!==scope.profile
      || catalog.repo_id!==scope.repo_id || catalog.repo!==scope.repo || Object.keys(catalog).length!==6
      || !Array.isArray(catalog.instances) || catalog.instances.length>128 || catalog.instances.some(instance=>{
        if(!instance || Object.keys(instance).length!==6 || !bounded(instance.backend_instance) || !bounded(instance.logical_backend)
          || ids.has(instance.backend_instance) || typeof instance.enabled!=='boolean' || typeof instance.executable_ready!=='boolean'
          || (instance.auth_ready!==null && typeof instance.auth_ready!=='boolean') || !Array.isArray(instance.supported_models)
          || instance.supported_models.length>256 || !instance.supported_models.every(bounded)) return true;
        ids.add(instance.backend_instance);return false;
      })) throw Error('Invalid worker account metadata');
    return catalog;
  }
  stage(binding:NativeDispatchBinding,contextDirectory:string):Promise<NativeWorkerObservation> {
    if(resolve(contextDirectory)!==join(this.root,binding.run_id,'context')) return Promise.reject(Error('Native context is outside its broker root'));
    const active=this.transfers.get(binding.run_id);if(active) return active;
    const transfer=this.transfer(binding,contextDirectory);
    this.transfers.set(binding.run_id,transfer);
    void transfer.finally(()=>{if(this.transfers.get(binding.run_id)===transfer)this.transfers.delete(binding.run_id);}).catch(()=>undefined);
    return transfer;
  }
  private async transfer(binding:NativeDispatchBinding,context:string):Promise<NativeWorkerObservation> {
    const fd=openNativeFile(join(context,'manifest.json'));let bytes:Buffer;
    try {bytes=readFileSync(fd);}finally{closeSync(fd);}
    const manifest=nativeContextManifest(binding,bytes);
    // This operation outlives a short broker observation deadline. Only the
    // fixed transfer/stage protocol retries; start never uses this path.
    for(let round=0;round<3;round++) {
      const prepared=await this.action(binding,'prepare') as {files:string[];stage_offered:boolean};
      if(typeof prepared?.stage_offered!=='boolean' || !Array.isArray(prepared.files) || prepared.files.length>11
        || prepared.files.some(name=>typeof name!=='string' || (name!=='manifest.json' && !Object.hasOwn(manifest.files,name)))) throw Error('Invalid native transfer request');
      if(prepared.stage_offered || prepared.files.length===0) return this.action(binding,'stage') as Promise<NativeWorkerObservation>;
      for(const name of prepared.files) {
        const source=openNativeFile(join(context,name),name.startsWith('history-')?128*1024*1024:8*1024*1024);
        const expected=name==='manifest.json'?bytes.length:manifest.files[name].bytes;
        if(fstatSync(source).size!==expected) {closeSync(source);throw Error('Native source file size changed');}
        const query=new URLSearchParams({run_id:binding.run_id,node_id:binding.node_id,context_sha256:binding.context_sha256,file:name});
        await this.request(binding,`/api/native-worker/context?${query}`,createReadStream('',{fd:source,autoClose:true}));
      }
    }
    throw Error('Native context transfer incomplete');
  }
  start(binding:NativeDispatchBinding,grant:NativeStartGrant,signal?:AbortSignal):Promise<NativeWorkerObservation> {
    return this.action(binding,'start',{grant},signal) as Promise<NativeWorkerObservation>;
  }
  async observe(binding:NativeDispatchBinding,signal?:AbortSignal):Promise<NativeWorkerObservation> {
    if(this.transfers.has(binding.run_id)) return {...binding,state:'unknown'};
    const prepared=await this.action(binding,'prepare',{},signal) as {stage_offered:boolean};
    if(prepared.stage_offered===false) {
      void this.stage(binding,join(this.root,binding.run_id,'context')).catch(()=>undefined);
      return {...binding,state:'unknown'};
    }
    if(prepared.stage_offered!==true) throw Error('Native stage observation unavailable');
    return this.action(binding,'observe',{},signal) as Promise<NativeWorkerObservation>;
  }
  releaseReservation(binding:NativeDispatchBinding,reservation:NativeHostReservation,signal?:AbortSignal):Promise<NativeReservationRelease> {
    return this.action(binding,'release-reservation',{reservation_id:reservation.id},signal) as Promise<NativeReservationRelease>;
  }
  releaseResult(binding:NativeDispatchBinding,_reservation:NativeHostReservation,signal?:AbortSignal):Promise<NativeWorkerRelease> {
    return this.action(binding,'release-result',{},signal) as Promise<NativeWorkerRelease>;
  }
}
