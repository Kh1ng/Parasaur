import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { test } from 'node:test';
import { nativeDispatchAuthority } from './nativeDispatchAuthority.js';
import type { NativeDispatchBinding, NativeHostReservation } from './nativeDispatchBroker.js';

function fixture() {
  const root=mkdtempSync(join(tmpdir(),'gah-native-authority-'));
  const loopFile=join(root,'loop.json'),policyFile=join(root,'policy.json'),entered=join(root,'entered');
  const binding: NativeDispatchBinding={run_id:'11111111-1111-4111-8111-111111111111',node_id:'worker',context_sha256:'a'.repeat(64),profile:'fixture',repo_id:'fixture',repo:'fixture/repo',work_id:'#1',mode:'fix',backend:'codex',backend_instance:null,model:'fixture-model',branch:null,review_generation:null,dispatch_reason:'initial'};
  const reservation: NativeHostReservation={...binding,id:'reservation',expires_at:new Date(Date.now()+20000).toISOString()};
  let loop={load:'loaded',state:'active',pid:123,started:'controller-instance-a',hold:false};
  let policy: {binding:NativeDispatchBinding;policy_current:boolean;work_current:boolean;execution_authorized:boolean;restart?:boolean;hold?:boolean}={binding,policy_current:true,work_current:true,execution_authorized:false};
  const save=()=>{writeFileSync(loopFile,JSON.stringify(loop));writeFileSync(policyFile,JSON.stringify(policy));}; save();
  writeFileSync(join(root,'systemctl'),`#!${process.execPath}\nconst s=JSON.parse(require('node:fs').readFileSync(${JSON.stringify(loopFile)},'utf8'));if(s.hold)setInterval(()=>{},1000);else console.log('LoadState='+s.load+'\\nActiveState='+s.state+'\\nMainPID='+s.pid+'\\nActiveEnterTimestamp='+s.started);`,{mode:0o700});
  const executable=join(root,'gah');
  writeFileSync(executable,`#!${process.execPath}\nconst fs=require('node:fs');const p=JSON.parse(fs.readFileSync(${JSON.stringify(policyFile)},'utf8'));if(!process.argv.includes('--check-work'))process.exit(91);if(p.restart){const s=JSON.parse(fs.readFileSync(${JSON.stringify(loopFile)},'utf8'));s.pid++;fs.writeFileSync(${JSON.stringify(loopFile)},JSON.stringify(s));}fs.writeFileSync(${JSON.stringify(entered)},'entered');if(p.hold)setInterval(()=>{},1000);else console.log(JSON.stringify(p));`,{mode:0o700});
  const saved={PATH:process.env.PATH,GAH_BINARY:process.env.GAH_BINARY};
  process.env.PATH=`${root}:${saved.PATH}`;process.env.GAH_BINARY=executable;
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

test('authority bounds simultaneous evidence snapshots and abort does not leave it permanently busy', {skip:process.platform==='win32',timeout:10000},async()=>{
  const f=fixture(),controller=new AbortController();
  try {
    f.policy({hold:true});const pending=f.check(f.binding,f.reservation,controller.signal);
    const deadline=Date.now()+5000;
    while(!existsSync(f.entered)){assert.ok(Date.now()<deadline,'Fixture CLI started');await new Promise(resolve=>setTimeout(resolve,10));}
    const other=nativeDispatchAuthority(f.root,'https://central.invalid');
    assert.deepEqual(await other(f.binding,f.reservation),{allowed:false,reason:'authority_busy'});
    controller.abort();assert.deepEqual(await pending,{allowed:false,reason:'authority_unavailable'});
    f.policy({hold:false});assert.deepEqual(await f.check(f.binding,f.reservation),{allowed:true});
  } finally {controller.abort();f.close();}
});
