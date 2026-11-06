import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash, randomUUID } from 'node:crypto';
import { chmodSync, existsSync, mkdtempSync, mkdirSync, readFileSync, unlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import type { NativeResultAcknowledgement } from '@git-agent-harness/contracts';
import { ClaimsService, ClaimConflictError } from './claimsService.js';
import { NativeDispatchBroker, type NativeAuthorization, type NativeDispatchBinding, type NativeHostReservation, type NativeWorkerObservation } from './nativeDispatchBroker.js';

const hash = (value: string) => createHash('sha256').update(value).digest('hex');
function fixture() {
  let now = Date.parse('2026-11-06T19:00:00Z');
  const root = mkdtempSync(join(tmpdir(), 'gah-native-broker-')); chmodSync(root, 0o700);
  const run = randomUUID(); const runPath = join(root, run); mkdirSync(runPath, { mode: 0o700 });
  const context = join(runPath, 'context'); mkdirSync(context, { mode: 0o700 });
  const request = { args: { run_id: run, profile: 'gah', work_id: '#1525', mode: 'fix', backend: 'opencode', model: 'fixture-model' } };
  const content: Record<string,string> = { 'request.json': JSON.stringify(request), 'policy.json': JSON.stringify({ fields: { repo_id: 'fixture', repo: 'fixture/repo' } }), 'history-0000.jsonl': '' };
  const files = Object.fromEntries(Object.entries(content).map(([name, bytes]) => { writeFileSync(join(context, name), bytes, { mode: 0o600 }); return [name, { sha256: hash(bytes), bytes: Buffer.byteLength(bytes) }]; }));
  const manifest = JSON.stringify({ version: 2, node_id: 'worker', run_id: run, profile: 'gah', central_url: 'https://central.invalid', history_chunks: 1, files });
  writeFileSync(join(context, 'manifest.json'), manifest, { mode: 0o600 });
  const binding: NativeDispatchBinding = { run_id: run, node_id: 'worker', context_sha256: hash(manifest), profile: 'gah', repo_id: 'fixture', repo: 'fixture/repo', work_id: '#1525', mode: 'fix', backend: 'opencode', model: 'fixture-model', backend_instance: null, branch: null, review_generation: null, dispatch_reason: null };
  writeFileSync(join(context, 'binding.json'), JSON.stringify(binding), { mode: 0o600 });
  const claimsPath = join(root, 'claims.json'); let claims = new ClaimsService(claimsPath, () => now);
  let stages = 0; let starts = 0; let releases = 0; let imports = 0;
  const reservation = (): NativeHostReservation => ({ ...binding, id: 'reservation', expires_at: new Date(now + 20_000).toISOString() });
  let observation: NativeWorkerObservation = { ...binding, state: 'ready', reservation: reservation() };
  let authorize: () => Promise<NativeAuthorization> = async () => ({ allowed: true, grant: { ...binding, reservation_id: 'reservation', expires_at: new Date(now + 10_000).toISOString(), token: 's'.repeat(64) } });
  let stageFails = false; let startFails = false; let importFails = false; let observeFails = false; let stageHangs = false; let timeout=5000;
  const ack: NativeResultAcknowledgement = { ...binding, profile: 'gah', work_id: '#1525', receipt_sha256: 'b'.repeat(64), outcome: 'recorded' };
  const deps = () => ({ root, centralUrl: 'https://central.invalid', coordinatorNodeId: 'central', claims, now: () => now, observationTimeoutMs:timeout,
    authorizeStart: () => authorize(),
    transport: {
      async stage() { stages++; assert.equal(JSON.parse(readFileSync(join(runPath,'broker.json'),'utf8')).stage_offered,true); if (stageHangs) return new Promise<NativeWorkerObservation>(() => undefined); if (stageFails) throw Error('PRIVATE_STAGE_TOKEN'); return observation; },
      async start() { starts++; assert.equal(JSON.parse(readFileSync(join(runPath,'broker.json'),'utf8')).start_offered,true); if (startFails) throw Error('PRIVATE_START_TOKEN'); observation = { ...binding,state: 'running' }; return observation; },
      async observe() { if (observeFails) throw Error('PRIVATE_OBSERVATION_TOKEN'); return observation; },
      async releaseReservation() { releases++; },
    },
    async recordCanonicalResult({ receiptFile }: { receiptFile: string }) { imports++; if (importFails) throw Error('PRIVATE_IMPORT_TOKEN'); return { ...ack, receipt_sha256: hash(readFileSync(receiptFile,'utf8')), outcome: imports > 1 ? 'already_recorded' as const : 'recorded' as const }; },
  });
  let broker = new NativeDispatchBroker(deps());
  return { root, run, runPath, context, binding, ack, request,
    get broker() { return broker; }, get claims() { return claims; }, get stages() { return stages; }, get starts() { return starts; }, get imports() { return imports; }, get releases() { return releases; },
    observe(value: NativeWorkerObservation) { observation = value; },
    failStage() { stageFails = true; }, failStart() { startFails = true; }, failImport() { importFails = true; }, recoverImport() { importFails = false; }, failObserve() { observeFails = true; },
    hangStage() { stageHangs=true; timeout=5; broker=new NativeDispatchBroker(deps()); },
    authorize(value: () => Promise<NativeAuthorization>) { authorize = value; }, advance() { now += 24 * 60 * 60_000; },
    reload() { claims = new ClaimsService(claimsPath, () => now); broker = new NativeDispatchBroker(deps()); return broker; },
  };
}

test('concurrent exact requests persist intent before transport and stage/start once', async () => {
  const f = fixture();
  const results = await Promise.all(Array.from({ length: 6 }, () => f.broker.offerPrepared(f.run, f.binding.context_sha256)));
  assert.ok(results.every(result => result.phase === 'running'));
  assert.equal(f.stages, 1); assert.equal(f.starts, 1);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  assert.equal(readFileSync(join(f.runPath,'broker.json'),'utf8').includes('s'.repeat(64)), false);
});

test('lost start acknowledgement, expiry, restart and stale ready observations never cause another launch', async () => {
  const f = fixture(); f.failStart();
  assert.equal((await f.broker.offerPrepared(f.run, f.binding.context_sha256)).phase, 'reconciling');
  f.advance(); f.reload();
  assert.equal((await f.broker.offerPrepared(f.run, f.binding.context_sha256)).phase, 'reconciling');
  assert.equal(f.stages, 1); assert.equal(f.starts, 1);
  assert.throws(() => f.claims.acquire('another','gah','#1525'), ClaimConflictError);
  assert.equal(readFileSync(join(f.runPath,'broker.json'),'utf8').includes('PRIVATE_START_TOKEN'), false);
});

test('lost staging acknowledgement and an absent worker preserve ownership without re-offering', async () => {
  const f = fixture(); f.failStage();
  await f.broker.offerPrepared(f.run, f.binding.context_sha256);
  f.observe({ ...f.binding, state: 'unknown' }); f.reload();
  assert.equal((await f.broker.reconcile(f.run)).phase, 'reconciling');
  assert.equal(f.stages,1); assert.equal(f.starts,0);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('denied, expired, mismatched and unbounded grants never start and release idle host reservations', async () => {
  for (const kind of ['denied','expired','wrong-context','too-long'] as const) {
    const f = fixture(); f.authorize(async () => kind === 'denied' ? { allowed:false, reason:'budget_held' } : { allowed:true, grant: {
      ...f.binding, context_sha256: kind === 'wrong-context' ? 'c'.repeat(64) : f.binding.context_sha256,
      reservation_id:'reservation',token:'s'.repeat(64),expires_at:kind === 'expired' ? '2026-11-06T18:00:00Z' : kind === 'too-long' ? '2026-11-06T20:00:00Z' : '2026-11-06T19:00:10Z',
    } });
    assert.equal((await f.broker.offerPrepared(f.run,f.binding.context_sha256)).phase,'authorization_deferred');
    assert.equal(f.starts,0); assert.equal(f.releases,1);
    assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  }
});

test('an observation deadline leaves the exact stage intent held and never repeats it', async () => {
  const f=fixture(); f.hangStage();
  assert.equal((await f.broker.offerPrepared(f.run,f.binding.context_sha256)).phase,'reconciling');
  f.observe({...f.binding,state:'unknown'}); f.reload();
  await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  assert.equal(f.stages,1); assert.equal(f.starts,0);
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('a missing journal cannot recreate a held run after a lost start acknowledgement', async () => {
  const f=fixture(); f.failStart(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  unlinkSync(join(f.runPath,'broker.json')); f.reload();
  await assert.rejects(f.broker.offerPrepared(f.run,f.binding.context_sha256),/ownership exists without its broker journal/);
  assert.equal(f.stages,1); assert.equal(f.starts,1);
});

test('capacity wait does not request authority or start; a later fresh reservation can proceed', async () => {
  const f = fixture(); let authorityCalls=0;
  f.observe({ ...f.binding,state:'waiting_capacity' });
  assert.equal((await f.broker.offerPrepared(f.run,f.binding.context_sha256)).phase,'waiting_capacity');
  f.authorize(async () => { authorityCalls++; return { allowed:false,reason:'factory_paused' }; });
  assert.equal(authorityCalls,0); assert.equal(f.starts,0);
  f.observe({ ...f.binding,state:'ready',reservation:{ ...f.binding,id:'new-reservation',expires_at:'2026-11-06T19:00:20Z' } });
  await f.broker.reconcile(f.run); assert.equal(authorityCalls,1); assert.equal(f.starts,0);
});

test('canonical import failure retains spooled result and ownership until replay succeeds', async () => {
  const f = fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.failImport(); f.observe({ ...f.binding,state:'terminal',receipt_json:'{"fixture":"result"}' });
  assert.equal((await f.broker.reconcile(f.run)).phase,'result_pending');
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  f.recoverImport(); f.failObserve(); f.reload();
  assert.equal((await f.broker.reconcile(f.run)).phase,'terminal');
  assert.equal(f.claims.getLease('gah','#1525'),undefined);
  assert.equal(existsSync(join(f.context,'history-0000.jsonl')),false);
  assert.equal(existsSync(join(f.context,'binding.json')),true);
  assert.equal((await f.broker.offerPrepared(f.run,f.binding.context_sha256)).phase,'terminal');
  assert.equal(f.starts,1); assert.equal(f.stages,1);
});

test('a terminal receipt before start can reconcile preflight failure but never restart that run', async () => {
  const f = fixture(); f.failImport(); f.observe({ ...f.binding,state:'terminal',receipt_json:'{"preflight":"failed"}' });
  await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.observe({ ...f.binding,state:'ready',reservation:{ ...f.binding,id:'reservation',expires_at:'2026-11-06T19:00:20Z' } });
  await f.broker.reconcile(f.run);
  assert.equal(f.starts,0); assert.equal(f.imports,2);
});

test('changed manifest-bound controls or binding metadata cannot reuse a prepared run', async () => {
  const f = fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  writeFileSync(join(f.context,'request.json'),JSON.stringify({ args:{...f.request.args,model:'other-model'} }),{mode:0o600});
  await assert.rejects(f.broker.offerPrepared(f.run,f.binding.context_sha256), /request or policy changed/);
  assert.equal(f.starts,1); assert.equal(f.stages,1);
});

test('stale running observations cannot postpone a saved terminal result', async () => {
  const f = fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.failImport(); f.observe({ ...f.binding,state:'terminal',receipt_json:'{"fixture":"finished"}' });
  assert.equal((await f.broker.reconcile(f.run)).phase,'result_pending');
  f.recoverImport(); f.observe({ ...f.binding,state:'running' }); f.reload();
  assert.equal((await f.broker.reconcile(f.run)).phase,'terminal');
  assert.equal(f.starts,1); assert.equal(f.claims.getLease('gah','#1525'),undefined);
});

test('a mismatched canonical context acknowledgement retains ownership', async () => {
  const f = fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.ack.context_sha256 = 'c'.repeat(64);
  f.observe({ ...f.binding,state:'terminal',receipt_json:'{"fixture":"finished"}' });
  assert.equal((await f.broker.reconcile(f.run)).phase,'result_pending');
  assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  assert.equal(existsSync(join(f.context,'history-0000.jsonl')),true);
});

test('terminal reconciliation retries deferred history cleanup without importing or starting again', async () => {
  const f = fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  const history = join(f.context,'history-0000.jsonl');
  // A directory in place of a history file must never be removed recursively.
  unlinkSync(history); mkdirSync(history,{ mode:0o700 });
  f.observe({ ...f.binding,state:'terminal',receipt_json:'{"fixture":"finished"}' });
  const done = await f.broker.reconcile(f.run);
  assert.equal(done.phase,'terminal'); assert.equal(done.reason,'history_cleanup_pending');
  const journal = JSON.parse(readFileSync(join(f.runPath,'broker.json'),'utf8'));
  assert.equal(journal.history_cleaned,false);
  // Simulate the operator resolving the invalid path without another job run.
  const { rmdirSync } = await import('node:fs'); rmdirSync(history);
  assert.equal((await f.reload().reconcile(f.run)).reason,undefined);
  assert.equal(JSON.parse(readFileSync(join(f.runPath,'broker.json'),'utf8')).history_cleaned,true);
  assert.equal(f.imports,1); assert.equal(f.starts,1);
});

test('an uncertain committed stage intent latches the broker and reload never repeats stage', async t => {
  const f=fixture(); const rename=fs.renameSync; let writes=0;
  t.mock.method(fs,'renameSync',(source: fs.PathLike,destination: fs.PathLike) => {
    rename(source,destination);
    if (String(destination)===join(f.runPath,'broker.json') && ++writes===3) throw Error('Injected post-rename failure');
  });
  syncBuiltinESMExports();
  try {
    await assert.rejects(f.broker.offerPrepared(f.run,f.binding.context_sha256),/post-rename failure/);
    await assert.rejects(f.broker.reconcile(f.run),/persistence is uncertain/);
    assert.equal(f.stages,0); assert.equal(f.starts,0);
  } finally { t.mock.restoreAll(); syncBuiltinESMExports(); }
  f.observe({...f.binding,state:'unknown'}); f.reload();
  assert.equal((await f.broker.offerPrepared(f.run,f.binding.context_sha256)).phase,'reconciling');
  assert.equal(f.stages,0); assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
});

test('uncertain receipt persistence retains ownership and reload recovers the same result', async t => {
  const f=fixture(); await f.broker.offerPrepared(f.run,f.binding.context_sha256);
  f.observe({...f.binding,state:'terminal',receipt_json:'{"fixture":"finished"}'});
  const rename=fs.renameSync;
  t.mock.method(fs,'renameSync',(source: fs.PathLike,destination: fs.PathLike) => {
    rename(source,destination);
    if (/receipt-[a-f0-9]{64}\.json$/.test(String(destination))) throw Error('Injected receipt sync failure');
  });
  syncBuiltinESMExports();
  try {
    await assert.rejects(f.broker.reconcile(f.run),/persistence is uncertain/);
    assert.equal(f.imports,0); assert.ok(f.claims.getLease('gah','#1525')?.native_handoff);
  } finally { t.mock.restoreAll(); syncBuiltinESMExports(); }
  assert.equal((await f.reload().reconcile(f.run)).phase,'terminal');
  assert.equal(f.imports,1); assert.equal(f.starts,1);
});
