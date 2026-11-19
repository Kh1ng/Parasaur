import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash, randomUUID } from 'node:crypto';
import { chmodSync, mkdtempSync, readFileSync, existsSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, parse } from 'node:path';
import { Readable } from 'node:stream';
import { NativeWorkerContext } from './nativeWorkerContext.js';
import { nativeRunPath, type NativeDispatchBinding } from './nativeDispatchBroker.js';

function fixture() {
  const root=mkdtempSync(join(tmpdir(),'gah-native-transfer-'));chmodSync(root,0o700);
  const contents:Record<string,string>={'request.json':'{"fixture":"request"}','policy.json':'{"fixture":"policy"}','history-0000.jsonl':'{"fixture":"history"}\n'};
  const hash=(bytes:string)=>createHash('sha256').update(bytes).digest('hex');
  const run=randomUUID();
  const manifest=JSON.stringify({version:2,run_id:run,node_id:'worker',profile:'fixture',history_chunks:1,
    files:Object.fromEntries(Object.entries(contents).map(([name,bytes])=>[name,{bytes:Buffer.byteLength(bytes),sha256:hash(bytes)}]))});
  const binding:NativeDispatchBinding={run_id:run,node_id:'worker',context_sha256:hash(manifest),profile:'fixture',
    repo:'fixture/repo',repo_id:'fixture',work_id:'#1',mode:'fix',backend:'codex',model:'fixture-model',backend_instance:null,
    branch:null,review_generation:null,dispatch_reason:'initial'};
  return {root,contents,manifest,binding,store:new NativeWorkerContext(root)};
}

test('native run paths reject traversal, absolute paths and trailing control characters', () => {
  const run=randomUUID(),root=resolve(tmpdir(),'native-path-boundary');
  assert.equal(nativeRunPath(root,run),join(root,run));
  assert.equal(nativeRunPath(parse(root).root,run),join(parse(root).root,run));
  for(const invalid of ['../escape','..\\escape',root,`${run}/../escape`,`${run}\n`,`${run}\0`,'00000000-0000-0000-0000-000000000000']) {
    assert.throws(()=>nativeRunPath(root,invalid),/Invalid native run/);
  }
});
test('streamed context resumes missing files and commits one immutable stage fence',async()=>{
  const f=fixture();assert.deepEqual(f.store.begin(f.binding),['manifest.json']);
  await f.store.accept(f.binding,'manifest.json',Readable.from([f.manifest]));
  assert.deepEqual(f.store.begin(f.binding).sort(),Object.keys(f.contents).sort());
  for(const [name,bytes] of Object.entries(f.contents)) await f.store.accept(f.binding,name,Readable.from([bytes]));
  assert.deepEqual(f.store.begin(f.binding),[]);assert.equal(f.store.offerStage(f.binding),true);
  const cold=new NativeWorkerContext(f.root);assert.equal(cold.offerStage(f.binding),false);
  assert.deepEqual(cold.binding(f.binding.run_id),f.binding);
  await assert.rejects(cold.accept(f.binding,'request.json',Readable.from(['replacement'])),/already offered/);
  assert.equal(readFileSync(join(cold.paths(f.binding).context,'request.json'),'utf8'),f.contents['request.json']);
});
test('altered manifest, oversized/corrupt file and paths never become committed context',async()=>{
  const f=fixture();f.store.begin(f.binding);
  await assert.rejects(f.store.accept(f.binding,'manifest.json',Readable.from(['{}'])),/digest or size/);
  await f.store.accept(f.binding,'manifest.json',Readable.from([f.manifest]));
  for(const [name,bytes] of [['../escape','private'],['request.json','wrong'],['policy.json','x'.repeat(100)]]) {
    await assert.rejects(f.store.accept(f.binding,name,Readable.from([bytes])));
    assert.equal(existsSync(join(f.store.paths(f.binding).context,name)),false);
  }
  assert.throws(()=>f.store.offerStage(f.binding),/incomplete/);
  assert.throws(()=>f.store.begin({...f.binding,work_id:'#2'}),/conflicts/);
  assert.throws(()=>f.store.binding('../escape'),/Invalid/);
});
test('stale partial transfers are removed only as private regular owned files',async()=>{
  const f=fixture();f.store.begin(f.binding);
  const path=join(f.store.paths(f.binding).context,'.manifest.json.upload');
  writeFileSync(path,'incomplete',{mode:0o600});
  await f.store.accept(f.binding,'manifest.json',Readable.from([f.manifest]));
  assert.equal(existsSync(path),false);
  await assert.rejects(f.store.accept(f.binding,'manifest.json',Readable.from([f.manifest])),/already exists/);
});
test('per-run exclusion rejects concurrent mutations and clears after failure',async()=>{
  const f=fixture();f.store.begin(f.binding);let unblock!:()=>void;
  const active=f.store.exclusive(f.binding,()=>new Promise<void>(resolve=>{unblock=resolve;}));
  await assert.rejects(f.store.exclusive(f.binding,async()=>undefined),/unavailable/);
  unblock();await active;
  await assert.rejects(f.store.exclusive(f.binding,async()=>{throw Error('fixture failure');}));
  await f.store.exclusive(f.binding,async()=>undefined);
});
