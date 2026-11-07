/** Immutable, streamed native context transfer. Remote input selects only a
 * run UUID and manifest-owned filenames, never a local path or command. */
import { createHash } from 'node:crypto';
import { constants, existsSync, fstatSync, lstatSync, mkdirSync, openSync, closeSync, readFileSync, fsyncSync, statfsSync } from 'node:fs';
import { open, rename, unlink } from 'node:fs/promises';
import { join, resolve, dirname } from 'node:path';
import { Transform, type Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { isDeepStrictEqual } from 'node:util';
import { writeFileAtomic } from './atomicFile.js';
import { isNativeRunId, type NativeDispatchBinding } from './nativeDispatchBroker.js';

const JSON_LIMIT=8*1024*1024, HISTORY_LIMIT=128*1024*1024;
const fields=['run_id','node_id','context_sha256','profile','repo_id','repo','work_id','mode','backend','backend_instance','model','branch','review_generation','dispatch_reason'] as const;
const nullable=new Set(['backend_instance','model','branch','review_generation','dispatch_reason']);
export function nativeBinding(value:unknown): value is NativeDispatchBinding {
  if(!value || typeof value!=='object' || Array.isArray(value)) return false;
  const binding=value as NativeDispatchBinding;
  return Object.keys(binding).length===fields.length && fields.every(key=>Object.hasOwn(binding,key)
    && ((nullable.has(key) && binding[key]===null) || (typeof binding[key]==='string' && binding[key]!.length>0
      && binding[key]!.length<=4096 && !/[\x00-\x1f\x7f]/.test(binding[key]!))))
    && isNativeRunId(binding.run_id) && /^[a-f0-9]{64}$/.test(binding.context_sha256)
    && typeof binding.model==='string' && !['auto','default'].includes(binding.model.toLowerCase());
}
function privateDirectory(path:string):void {
  const info=lstatSync(path);
  if(!info.isDirectory() || info.isSymbolicLink() || (process.platform!=='win32'
    && (info.uid!==process.getuid!() || (info.mode&0o077)!==0))) throw Error('Native directory is not private');
}
function syncDirectory(path:string):void {
  if(process.platform==='win32') return;
  const fd=openSync(path,constants.O_RDONLY);try{fsyncSync(fd);}finally{closeSync(fd);}
}
export function openNativeFile(path:string,limit=JSON_LIMIT,flags:number=constants.O_RDONLY):number {
  const fd=openSync(path,flags|(constants.O_NOFOLLOW??0));
  try {
    const info=fstatSync(fd);
    if(!info.isFile() || info.nlink!==1 || info.size>limit || (process.platform!=='win32'
      && (info.uid!==process.getuid!() || (info.mode&0o077)!==0))) throw Error('Native file is not private and bounded');
    return fd;
  } catch(error) {closeSync(fd);throw error;}
}
function readPrivate(path:string,limit=JSON_LIMIT):Buffer {
  const fd=openNativeFile(path,limit);try{return readFileSync(fd);}finally{closeSync(fd);}
}
interface Manifest {version:number;run_id:string;node_id:string;profile:string;history_chunks:number;files:Record<string,{bytes:number;sha256:string}>}
export function nativeContextManifest(binding:NativeDispatchBinding,raw:Buffer):Manifest {
  if(raw.length>JSON_LIMIT || createHash('sha256').update(raw).digest('hex')!==binding.context_sha256) throw Error('Native manifest digest changed');
  const manifest=JSON.parse(raw.toString()) as Manifest;
  if(manifest.version!==2 || manifest.run_id!==binding.run_id || manifest.node_id!==binding.node_id
    || manifest.profile!==binding.profile || !Number.isInteger(manifest.history_chunks)
    || manifest.history_chunks<1 || manifest.history_chunks>8 || !manifest.files || typeof manifest.files!=='object') throw Error('Invalid native manifest');
  const expected=['request.json','policy.json',...Array.from({length:manifest.history_chunks},(_,index)=>`history-${String(index).padStart(4,'0')}.jsonl`)];
  if(!isDeepStrictEqual(Object.keys(manifest.files).sort(),expected.sort()) || expected.some(name=>{
    const file=manifest.files[name];return !file || !Number.isSafeInteger(file.bytes) || file.bytes<0
      || file.bytes>(name.startsWith('history-')?HISTORY_LIMIT:JSON_LIMIT) || !/^[a-f0-9]{64}$/.test(file.sha256);
  })) throw Error('Invalid native manifest files');
  return manifest;
}

export class NativeWorkerContext {
  readonly root:string;
  private readonly active=new Set<string>();
  constructor(root:string) {
    this.root=resolve(root);
    if(!existsSync(this.root)) {mkdirSync(this.root,{recursive:true,mode:0o700});syncDirectory(dirname(this.root));}
    privateDirectory(this.root);
  }
  async exclusive<T>(binding:NativeDispatchBinding,operation:()=>Promise<T>):Promise<T> {
    if(!nativeBinding(binding) || this.active.has(binding.run_id)) throw Error('Native worker operation unavailable');
    this.active.add(binding.run_id);try{return await operation();}finally{this.active.delete(binding.run_id);}
  }
  binding(runId:string):NativeDispatchBinding {
    if(!isNativeRunId(runId)) throw Error('Invalid native run identity');
    const run=join(this.root,runId);privateDirectory(run);
    const binding=JSON.parse(readPrivate(join(run,'binding.json')).toString());
    if(!nativeBinding(binding) || binding.run_id!==runId) throw Error('Invalid stored native identity');
    this.paths(binding);return binding;
  }
  paths(binding:NativeDispatchBinding) {
    if(!nativeBinding(binding)) throw Error('Invalid native binding');
    const run=join(this.root,binding.run_id);privateDirectory(run);
    const stored=JSON.parse(readPrivate(join(run,'binding.json')).toString());
    if(!isDeepStrictEqual(stored,binding)) throw Error('Native binding changed');
    const context=join(run,'context');privateDirectory(context);
    return {run,context,work:join(run,'work'),bindingFile:join(run,'binding.json')};
  }
  begin(binding:NativeDispatchBinding):string[] {
    if(!nativeBinding(binding)) throw Error('Invalid native binding');
    const run=join(this.root,binding.run_id);
    if(!existsSync(run)) {mkdirSync(run,{mode:0o700});syncDirectory(this.root);}
    privateDirectory(run);
    const bindingFile=join(run,'binding.json');
    if(!existsSync(bindingFile)) {
      if(existsSync(join(run,'context')) || existsSync(join(run,'stage-offered.json'))) throw Error('Native transfer identity missing');
      writeFileAtomic(bindingFile,JSON.stringify(binding),0o600,process.platform!=='win32');
    }
    const stored=JSON.parse(readPrivate(bindingFile).toString());
    if(!isDeepStrictEqual(stored,binding)) throw Error('Native transfer conflicts with original run');
    const context=join(run,'context');
    if(!existsSync(context)) {mkdirSync(context,{mode:0o700});syncDirectory(run);}
    privateDirectory(context);
    if(this.stageOffered(binding)) return [];
    if(!existsSync(join(context,'manifest.json'))) return ['manifest.json'];
    return Object.keys(this.manifest(binding).files).filter(name=>!existsSync(join(context,name)));
  }
  stageOffered(binding:NativeDispatchBinding):boolean {
    const {run}=this.paths(binding);const path=join(run,'stage-offered.json');
    if(!existsSync(path)) return false;
    if(!isDeepStrictEqual(JSON.parse(readPrivate(path).toString()),binding)) throw Error('Native stage fence changed');
    return true;
  }
  offerStage(binding:NativeDispatchBinding):boolean {
    if(this.stageOffered(binding)) return false;
    if(this.begin(binding).length) throw Error('Native context transfer incomplete');
    // Full hashes/policy are independently verified by the Rust admission CLI.
    syncDirectory(this.paths(binding).context);
    writeFileAtomic(join(this.paths(binding).run,'stage-offered.json'),JSON.stringify(binding),0o600,process.platform!=='win32');
    return true;
  }
  private manifest(binding:NativeDispatchBinding,bytes?:Buffer):Manifest {
    const raw=bytes??readPrivate(join(this.paths(binding).context,'manifest.json'));
    return nativeContextManifest(binding,raw);
  }
  async accept(binding:NativeDispatchBinding,name:string,input:Readable):Promise<void> {
    const {context}=this.paths(binding);
    if(this.stageOffered(binding)) throw Error('Native stage already offered');
    const expected=name==='manifest.json'?{bytes:undefined,sha256:binding.context_sha256}:this.manifest(binding).files[name];
    if(!expected || !/^(manifest|request|policy)\.json$|^history-\d{4}\.jsonl$/.test(name)) throw Error('Unknown native context file');
    const destination=join(context,name),temporary=join(context,`.${name}.upload`);
    if(existsSync(destination)) throw Error('Native context file already exists');
    // Single API writer, per-run exclusion: a prior process's partial upload
    // may be removed, but never follow a symlink or touch another filename.
    if(existsSync(temporary)) {closeSync(openNativeFile(temporary,name.startsWith('history-')?HISTORY_LIMIT:JSON_LIMIT));await unlink(temporary);}
    const file=await open(temporary,'wx',0o600);let count=0;const hash=createHash('sha256');
    const limit=expected.bytes??JSON_LIMIT;
    try {
      const check=new Transform({transform(chunk:Buffer,_encoding,callback){
        count+=chunk.length;if(count>limit) return callback(Error('Native transfer exceeds its bound'));
        hash.update(chunk);callback(null,chunk);
      }});
      await pipeline(input,check,file.createWriteStream());
      if((expected.bytes!==undefined && count!==expected.bytes) || hash.digest('hex')!==expected.sha256) throw Error('Native transfer digest or size changed');
      if(name==='manifest.json') {
        const manifest=this.manifest(binding,readPrivate(temporary));
        const total=Object.values(manifest.files).reduce((sum,file)=>sum+BigInt(file.bytes),0n);
        const disk=statfsSync(context,{bigint:true});
        if(disk.bavail*disk.bsize<total*2n+64n*1024n*1024n) throw Error('Native context requires more free disk space');
      }
      const durable=openNativeFile(temporary,limit,constants.O_RDWR);
      try {fsyncSync(durable);}finally{closeSync(durable);}
      await rename(temporary,destination);syncDirectory(context);
    } finally {
      await file.close().catch(()=>undefined);
      await unlink(temporary).catch(()=>undefined);
    }
  }
}
