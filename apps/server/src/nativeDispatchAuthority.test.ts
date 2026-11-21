import assert from 'node:assert/strict';
import fs, { mkdirSync, mkdtempSync, writeFileSync, readFileSync, readdirSync, existsSync, rmSync } from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { test } from 'node:test';
import type { NativeHeldWork, NativeWorkerCatalog } from '@git-agent-harness/contracts';
import { nativeDispatchAuthority, nativeContinuationAuthority } from './nativeDispatchAuthority.js';
import type { NativeDispatchBinding, NativeHostReservation } from './nativeDispatchBroker.js';

function fixture() {
  const root=mkdtempSync(join(tmpdir(),'gah-native-authority-'));
  const loopFile=join(root,'loop.json'),policyFile=join(root,'policy.json'),entered=join(root,'entered');
  const binding: NativeDispatchBinding={run_id:'11111111-1111-4111-8111-111111111111',node_id:'worker',context_sha256:'a'.repeat(64),profile:'fixture',repo_id:'fixture',repo:'fixture/repo',work_id:'#1',mode:'fix',backend:'codex',backend_instance:null,model:'fixture-model',branch:null,review_generation:null,dispatch_reason:'initial'};
  const reservation: NativeHostReservation={...binding,id:'reservation',expires_at:new Date(Date.now()+20000).toISOString()};
  let loop={load:'loaded',state:'active',pid:123,started:'controller-instance-a',hold:false};
  let policy: {binding:NativeDispatchBinding;policy_current:boolean;work_current:boolean;route_current:boolean;execution_authorized:boolean;restart?:boolean;hold?:boolean}={binding,policy_current:true,work_current:true,route_current:true,execution_authorized:false};
  const save=()=>{writeFileSync(loopFile,JSON.stringify(loop));writeFileSync(policyFile,JSON.stringify(policy));}; save();
  writeFileSync(join(root,'systemctl'),`#!${process.execPath}\nconst s=JSON.parse(require('node:fs').readFileSync(${JSON.stringify(loopFile)},'utf8'));if(s.hold)setInterval(()=>{},1000);else console.log('LoadState='+s.load+'\\nActiveState='+s.state+'\\nMainPID='+s.pid+'\\nActiveEnterTimestamp='+s.started);`,{mode:0o700});
  const executable=join(root,'gah');
  writeFileSync(executable,`#!${process.execPath}\nconst fs=require('node:fs');const p=JSON.parse(fs.readFileSync(${JSON.stringify(policyFile)},'utf8'));const work=process.argv.includes('--check-work'),route=process.argv.includes('--check-route');if(work===route)process.exit(91);if(p.restart){const s=JSON.parse(fs.readFileSync(${JSON.stringify(loopFile)},'utf8'));s.pid++;fs.writeFileSync(${JSON.stringify(loopFile)},JSON.stringify(s));}fs.writeFileSync(${JSON.stringify(entered)},work?'work':'route');if(p.hold&&work)setInterval(()=>{},1000);else console.log(JSON.stringify(p));`,{mode:0o700});
  const saved={PATH:process.env.PATH,PARASAUR_BINARY:process.env.PARASAUR_BINARY};
  process.env.PATH=`${root}:${saved.PATH}`;process.env.PARASAUR_BINARY=executable;
  return {root,binding,reservation,entered,check:nativeDispatchAuthority(root,'https://central.invalid',join(root,'fixture.toml')),
    loop(changes:Partial<typeof loop>){loop={...loop,...changes};save();},
    policy(changes:Partial<typeof policy>){policy={...policy,...changes};save();},
    close(){for(const [key,value]of Object.entries(saved))if(value===undefined)delete process.env[key];else process.env[key]=value;rmSync(root,{recursive:true,force:true});},
  };
}

test('concrete authority requires an active managed controller and complete current work evidence', {skip:process.platform==='win32'},async()=>{
  const f=fixture();
  try {
    assert.deepEqual(await f.check(f.binding,f.reservation),{allowed:true});
    for(const state of ['activating','deactivating','inactive','failed']) {
      f.loop({state});assert.deepEqual(await f.check(f.binding,f.reservation),{allowed:false,reason:'factory_paused'});
    }
    f.loop({state:'active',pid:0});assert.deepEqual(await f.check(f.binding,f.reservation),{allowed:false,reason:'authority_unavailable'});
    f.loop({pid:123});f.policy({work_current:false});
    assert.deepEqual(await f.check(f.binding,f.reservation),{allowed:false,reason:'authority_unavailable'});
    f.policy({work_current:true,binding:{...f.binding,model:'different-model'}});
    assert.deepEqual(await f.check(f.binding,f.reservation),{allowed:false,reason:'policy_changed'});
    f.policy({binding:f.binding,restart:true});
    assert.deepEqual(await f.check(f.binding,f.reservation),{allowed:false,reason:'controller_changed'});
  } finally {f.close();}
});

test('an unavailable systemd bus is bounded and remains unknown rather than a reported pause', {skip:process.platform==='win32',timeout:10000},async()=>{
  const f=fixture();try {
    f.loop({hold:true});
    assert.deepEqual(await f.check(f.binding,f.reservation),{allowed:false,reason:'authority_unavailable'});
    f.loop({hold:false});assert.deepEqual(await f.check(f.binding,f.reservation),{allowed:true});
  } finally {f.close();}
});

test('continuation checks current policy and exact route without reselecting the next queued job',{skip:process.platform==='win32'},async()=>{
  const f=fixture(),check=nativeContinuationAuthority(f.root,'https://central.invalid');
  try {
    f.policy({work_current:false});
    assert.deepEqual(await check(f.binding,f.reservation),{allowed:true});
    assert.equal(readFileSync(f.entered,'utf8'),'route');
    f.policy({route_current:false});assert.deepEqual(await check(f.binding,f.reservation),{allowed:false,reason:'authority_unavailable'});
    f.policy({route_current:true,binding:{...f.binding,backend_instance:'different'}});
    assert.deepEqual(await check(f.binding,f.reservation),{allowed:false,reason:'policy_changed'});
    f.policy({binding:f.binding});f.loop({state:'inactive'});
    assert.deepEqual(await check(f.binding,f.reservation),{allowed:false,reason:'factory_paused'});
  } finally {f.close();}
});

test('the worker catalog reaches the check as a private file that is never flushed to disk and is removed afterwards',{skip:process.platform==='win32'},async(t)=>{
  const f=fixture(),run=join(f.root,f.binding.run_id),seen=join(f.root,'seen.json');
  try {
    mkdirSync(run);
    const catalog={node_id:'worker',protocol:2} as unknown as NativeWorkerCatalog;
    const held=():NativeHeldWork[]=>[{profile:'fixture',work_id:'#1',run_id:f.binding.run_id,node_id:'worker',context_sha256:f.binding.context_sha256,reserves_intake:true}];
    // This CLI records what it was handed, then answers like the fixture's.
    writeFileSync(join(f.root,'gah'),`#!${process.execPath}\nconst fs=require('node:fs');const file=process.argv[process.argv.indexOf('--worker-catalog-file')+1];fs.writeFileSync(${JSON.stringify(seen)},JSON.stringify({file,mode:fs.statSync(file).mode&0o777,body:fs.readFileSync(file,'utf8')}));console.log(fs.readFileSync(${JSON.stringify(join(f.root,'policy.json'))},'utf8'));`,{mode:0o700});
    const check=nativeContinuationAuthority(f.root,'https://central.invalid',join(f.root,'fixture.toml'),async()=>catalog,held);
    // Each flush stops the server's only thread until the disk has committed.
    // On a busy host that was most of the five seconds a worker waits.
    let flushes=0;
    t.mock.method(fs,'fsyncSync',()=>{flushes++;});syncBuiltinESMExports();
    try {assert.deepEqual(await check(f.binding,f.reservation),{allowed:true});}
    finally {t.mock.restoreAll();syncBuiltinESMExports();}
    const handed=JSON.parse(readFileSync(seen,'utf8'));
    assert.equal(dirname(handed.file),run);
    assert.equal(handed.mode,0o600);
    assert.deepEqual(JSON.parse(handed.body),{catalog,held_work:held()});
    assert.equal(flushes,0);
    assert.deepEqual(readdirSync(run),[]);
  } finally {f.close();}
});

test('authority bounds simultaneous evidence snapshots and abort does not leave it permanently busy', {skip:process.platform==='win32',timeout:10000},async()=>{
  const f=fixture(),controller=new AbortController();
  try {
    f.policy({hold:true});const pending=f.check(f.binding,f.reservation,controller.signal);
    const deadline=Date.now()+5000;
    while(!existsSync(f.entered)){assert.ok(Date.now()<deadline,'Fixture CLI started');await new Promise(resolve=>setTimeout(resolve,10));}
    // A second selection check waits for nothing and is refused; a route
    // check builds no snapshot and is answered while the first still runs.
    const second=nativeDispatchAuthority(f.root,'https://central.invalid',join(f.root,'fixture.toml'));
    assert.deepEqual(await second(f.binding,f.reservation),{allowed:false,reason:'authority_busy'});
    const route=nativeContinuationAuthority(f.root,'https://central.invalid',join(f.root,'fixture.toml'));
    assert.deepEqual(await route(f.binding,f.reservation),{allowed:true});
    controller.abort();assert.deepEqual(await pending,{allowed:false,reason:'authority_unavailable'});
    f.policy({hold:false});assert.deepEqual(await f.check(f.binding,f.reservation),{allowed:true});
  } finally {controller.abort();f.close();}
});
