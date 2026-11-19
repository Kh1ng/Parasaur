import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash, randomUUID } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { NativeWorker } from './nativeWorker.js';
import type { NativeDispatchBinding, NativeWorkerObservation } from './nativeDispatchBroker.js';

const linuxOnly={skip:process.platform!=='linux'?'native workers require Linux':false};

/** A worker whose launcher is a script that records each call and exits with
 * the code in `launcher.exit`, and whose CLI always fails, so every
 * observation is `unknown`. `restart` gives a second server on the same root. */
async function fixture() {
  const root=mkdtempSync(join(tmpdir(),'gah-native-worker-'));chmodSync(root,0o700);
  const tools=mkdtempSync(join(tmpdir(),'gah-native-tools-'));
  const calls=join(tools,'launcher.calls'),exit=join(tools,'launcher.exit');
  const launcher=join(tools,'launcher'),binary=join(tools,'gah');
  writeFileSync(launcher,`#!/bin/sh\necho launched >> '${calls}'\nexit "$(cat '${exit}')"\n`,{mode:0o700});
  writeFileSync(binary,'#!/bin/sh\nexit 1\n',{mode:0o700});
  writeFileSync(exit,'0');
  const contents:Record<string,string>={'request.json':'{"fixture":"request"}','policy.json':'{"fixture":"policy"}','history-0000.jsonl':'{"fixture":"history"}\n'};
  const hash=(bytes:string)=>createHash('sha256').update(bytes).digest('hex');
  const run=randomUUID();
  const manifest=JSON.stringify({version:2,run_id:run,node_id:'worker',profile:'fixture',history_chunks:1,
    files:Object.fromEntries(Object.entries(contents).map(([name,bytes])=>[name,{bytes:Buffer.byteLength(bytes),sha256:hash(bytes)}]))});
  const binding:NativeDispatchBinding={run_id:run,node_id:'worker',context_sha256:hash(manifest),profile:'fixture',
    repo:'fixture/repo',repo_id:'fixture',work_id:'#1',mode:'fix',backend:'codex',model:'fixture-model',backend_instance:null,
    branch:null,review_generation:null,dispatch_reason:'initial'};
  const restart=()=>new NativeWorker({root,nodeId:'worker',binary,launcher,centralToken:()=>undefined,
    profiles:async()=>[{name:'fixture',repo:'fixture/repo',repo_id:'fixture'}] as never});
  const worker=restart();
  worker.contexts.begin(binding);
  for(const [name,bytes] of Object.entries({'manifest.json':manifest,...contents})) await worker.contexts.accept(binding,name,Readable.from([bytes]));
  return {binding,worker,restart,
    launches:()=>existsSync(calls)?readFileSync(calls,'utf8').split('\n').filter(Boolean).length:0,
    launcherExits:(code:number)=>writeFileSync(exit,String(code)),
    admissionBegins:()=>mkdirSync(worker.contexts.paths(binding).work,{mode:0o700})};
}
const state=async(reply:Promise<unknown>)=>(await reply as NativeWorkerObservation).state;

test('a stage offer recorded before the server stopped is launched on the next observation',linuxOnly,async()=>{
  const f=await fixture();
  // The fence is durable but the server stopped before it started the holder.
  assert.equal(f.worker.contexts.offerStage(f.binding),true);
  const restarted=f.restart();
  assert.equal(await state(restarted.action(f.binding,'observe',{})),'unknown');
  assert.equal(f.launches(),1);
  // Once admission owns the job directory, it alone decides the run's state.
  f.admissionBegins();
  assert.equal(await state(restarted.action(f.binding,'observe',{})),'unknown');
  assert.equal(await state(restarted.action(f.binding,'stage',{})),'unknown');
  assert.equal(f.launches(),1);
});

test('a failed launch is reported once and repeated, and a refused repeat is only an observation',linuxOnly,async()=>{
  const f=await fixture();
  f.launcherExits(1);
  await assert.rejects(f.worker.action(f.binding,'stage',{}),/unconfirmed/);
  assert.equal(f.launches(),1);
  // systemd refuses a repeat while the first unit is still loaded.
  assert.equal(await state(f.worker.action(f.binding,'observe',{})),'unknown');
  assert.equal(f.launches(),2);
  f.launcherExits(0);
  assert.equal(await state(f.worker.action(f.binding,'stage',{})),'unknown');
  assert.equal(f.launches(),3);
});

test('an observation before any stage offer launches nothing',linuxOnly,async()=>{
  const f=await fixture();
  assert.equal(await state(f.worker.action(f.binding,'observe',{})),'unknown');
  assert.equal(f.launches(),0);
});
