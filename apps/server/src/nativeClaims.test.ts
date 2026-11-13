import assert from 'node:assert/strict';
import { test } from 'node:test';
import { randomUUID } from 'node:crypto';
import { mkdtempSync, mkdirSync, readFileSync, renameSync, rmdirSync, writeFileSync } from 'node:fs';
import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import type { NativeClaimBinding, NativeResultAcknowledgement } from '@git-agent-harness/contracts';
import { ClaimsService, ClaimConflictError } from './claimsService.js';

function fixture() {
  let clock = Date.parse('2026-11-06T18:00:00Z');
  const path = join(mkdtempSync(join(tmpdir(), 'gah-native-claims-')), 'claims.json');
  const binding: NativeClaimBinding = { run_id: randomUUID(), node_id: 'worker', context_sha256: 'a'.repeat(64) };
  const service = new ClaimsService(path, () => clock);
  const ack: NativeResultAcknowledgement = { ...binding, profile: 'gah', work_id: '#1525', receipt_sha256: 'b'.repeat(64), outcome: 'recorded' };
  return { path, binding, service, ack, advance: () => { clock += 24 * 60 * 60 * 1000; }, reload: () => new ClaimsService(path, () => clock) };
}

test('native ownership survives expiry, release, aliases and service reload without renewing authority', () => {
  const f = fixture();
  const lease = f.service.reserveNative('central', 'gah', '#1525', f.binding);
  f.service.observeNative('central', 'gah', 'TICKET-01525', f.binding, 'running');
  f.advance();
  f.service.release('central', 'gah', '1525');
  for (const service of [f.service, f.reload()]) {
    assert.equal(service.getLease('gah', '#1525')?.native_handoff?.phase, 'running');
    assert.throws(() => service.acquire('worker', 'gah', '#1525'), ClaimConflictError);
    assert.throws(() => service.acquire('central', 'gah', '#1525'), ClaimConflictError);
    assert.throws(() => service.renew('central', 'gah', '#1525'), ClaimConflictError);
    const replay = service.reserveNative('central', 'gah', 'TICKET-1525', f.binding);
    assert.equal(replay.expires_at, lease.expires_at, 'reservation replay is not renewed execution authority');
  }
});

test('changed runs, contexts, nodes and work identities cannot reuse a native reservation', () => {
  const f = fixture(); f.service.reserveNative('central', 'gah', '#1525', f.binding);
  for (const changed of [{ ...f.binding, run_id: randomUUID() }, { ...f.binding, node_id: 'other' }, { ...f.binding, context_sha256: 'c'.repeat(64) }]) {
    assert.throws(() => f.service.reserveNative('central', 'gah', '#1525', changed), ClaimConflictError);
    assert.throws(() => f.service.observeNative('central', 'gah', '#1525', changed, 'running'), ClaimConflictError);
  }
  assert.throws(() => f.service.reserveNative('central', 'gah', '#other', f.binding), ClaimConflictError);
  assert.throws(() => f.service.reserveNative('other-owner', 'gah', '#1525', f.binding), ClaimConflictError);
  const lease = f.service.getLease('gah', '#1525')!;
  lease.native_handoff!.run_id = randomUUID();
  assert.equal(f.service.getLease('gah', '#1525')!.native_handoff!.run_id, f.binding.run_id);
});

test('native work never adopts an existing legacy lease, including one held by central', () => {
  const f = fixture(); f.service.acquire('central', 'gah', '#1525');
  assert.throws(() => f.service.reserveNative('central', 'gah', '#1525', f.binding), ClaimConflictError);
  f.service.release('central', 'gah', '#1525');
  f.service.reserveNative('central', 'gah', '#1525', f.binding);
});

test('canonical import failure or a mismatched acknowledgement retains durable pending ownership', async () => {
  const f = fixture(); f.service.reserveNative('central', 'gah', '#1525', f.binding);
  await assert.rejects(f.service.completeNative('central', 'gah', '#1525', f.binding, async () => { throw Error('ledger not synchronized'); }));
  assert.equal(f.reload().getLease('gah', '#1525')?.native_handoff?.phase, 'result_pending');
  for (const wrong of [{ ...f.ack, node_id: 'other' }, { ...f.ack, run_id: randomUUID() }, { ...f.ack, work_id: '#else' }, { ...f.ack, context_sha256: 'c'.repeat(64) }, { ...f.ack, receipt_sha256: 'unknown' }]) {
    await assert.rejects(f.service.completeNative('central', 'gah', '#1525', f.binding, async () => wrong));
    assert.ok(f.reload().getLease('gah', '#1525')?.native_handoff);
  }
});

test('ownership remains held while canonical import is awaiting acknowledgement', async () => {
  const f = fixture(); f.service.reserveNative('central', 'gah', '#1525', f.binding);
  let acknowledge!: (value: NativeResultAcknowledgement) => void;
  const completing = f.service.completeNative('central', 'gah', '#1525', f.binding, () => new Promise(resolve => { acknowledge = resolve; }));
  assert.equal(f.reload().getLease('gah', '#1525')?.native_handoff?.phase, 'result_pending');
  assert.throws(() => f.service.acquire('worker', 'gah', '#1525'), ClaimConflictError);
  acknowledge(f.ack); await completing;
  assert.equal(f.reload().getLease('gah', '#1525'), undefined);
});

test('late canonical receipt replay never releases a newer reservation or legacy claim', async () => {
  const f = fixture(); f.service.reserveNative('central', 'gah', '#1525', f.binding);
  let acknowledgeLate!: (value: NativeResultAcknowledgement) => void;
  const late = f.service.completeNative('central', 'gah', '#1525', f.binding, () => new Promise(resolve => { acknowledgeLate = resolve; }));
  await f.service.completeNative('central', 'gah', '#1525', f.binding, async () => f.ack);
  const newer = { ...f.binding, run_id: randomUUID() };
  f.service.reserveNative('central', 'gah', '#1525', newer);
  acknowledgeLate({ ...f.ack, outcome: 'already_recorded' }); await late;
  await f.service.completeNative('central', 'gah', '#1525', f.binding, async () => ({ ...f.ack, outcome: 'already_recorded' }));
  assert.equal(f.reload().getLease('gah', '#1525')?.native_handoff?.run_id, newer.run_id);
});

test('a release persistence failure after canonical import remains uncertain until replayed reconciliation', async () => {
  const f = fixture(); f.service.reserveNative('central', 'gah', '#1525', f.binding);
  const saved = `${f.path}.saved`;
  await assert.rejects(f.service.completeNative('central', 'gah', '#1525', f.binding, async () => {
    renameSync(f.path, saved); mkdirSync(f.path); return f.ack;
  }));
  assert.throws(() => f.service.getLeases(), /persistence is uncertain/);
  rmdirSync(f.path); renameSync(saved, f.path);
  const recovered = f.reload();
  assert.throws(() => recovered.acquire('worker', 'gah', '#1525'), ClaimConflictError);
  await recovered.completeNative('central', 'gah', '#1525', f.binding, async () => ({ ...f.ack, outcome: 'already_recorded' }));
  assert.equal(f.reload().getLease('gah', '#1525'), undefined);
});

test('separators in profile and work IDs cannot collide in the claim store', () => {
  const f = fixture();
  f.service.acquire('one', 'a::b', 'c');
  f.service.acquire('two', 'a', 'b::c');
  assert.equal(f.reload().getLease('a::b', 'c')?.node_id, 'one');
  assert.equal(f.reload().getLease('a', 'b::c')?.node_id, 'two');
});

test('a write failure latches the service unavailable instead of granting from uncommitted memory', () => {
  const f = fixture(); f.service.reserveNative('central', 'gah', '#1525', f.binding);
  const saved = `${f.path}.saved`; renameSync(f.path, saved); mkdirSync(f.path);
  assert.throws(() => f.service.acquire('worker', 'gah', '#other'));
  assert.throws(() => f.service.getLeases(), /persistence is uncertain/);
  assert.throws(() => f.service.release('central', 'gah', '#1525'), /persistence is uncertain/);
  rmdirSync(f.path); renameSync(saved, f.path);
  assert.ok(f.reload().getLease('gah', '#1525')?.native_handoff);
});

test('a directory-sync failure after rename preserves the uncertain disk commit and blocks old-memory grants', { skip: process.platform === 'win32' }, t => {
  const f = fixture(); f.service.reserveNative('central', 'gah', '#1525', f.binding);
  const synchronize = fs.fsyncSync;
  t.mock.method(fs, 'fsyncSync', (descriptor: number) => {
    if (fs.fstatSync(descriptor).isDirectory()) throw Error('fixture directory sync failure');
    synchronize(descriptor);
  });
  syncBuiltinESMExports();
  try {
    assert.throws(() => f.service.acquire('worker', 'gah', '#other'), /directory sync failure/);
    assert.ok(JSON.parse(readFileSync(f.path, 'utf8')).fenced_leases.some((lease: { work_id: string }) => lease.work_id === '#other'));
    assert.throws(() => f.service.getLeases(), /persistence is uncertain/);
    assert.throws(() => f.service.acquire('another', 'gah', '#other'), /persistence is uncertain/);
    assert.throws(() => f.reload().reserveNative('central', 'gah', '#1525', f.binding), /directory sync failure/);
  } finally {
    t.mock.restoreAll(); syncBuiltinESMExports();
  }
  assert.throws(() => f.reload().acquire('another', 'gah', '#other'), ClaimConflictError);
  assert.ok(f.reload().getLease('gah', '#1525')?.native_handoff);
});

test('malformed native holds and duplicate canonical claim identities are never silently dropped', () => {
  const f = fixture(); f.service.reserveNative('central', 'gah', '#1525', f.binding);
  const data = JSON.parse(readFileSync(f.path, 'utf8'));
  data.fenced_leases[0].native_handoff = null; writeFileSync(f.path, JSON.stringify(data));
  assert.throws(() => f.reload(), /Failed to load claims config/);
  data.fenced_leases = [{ profile: 'gah', work_id: 'TICKET-001525' }, { profile: 'gah', work_id: '#1525' }];
  writeFileSync(f.path, JSON.stringify(data));
  assert.throws(() => f.reload(), /Duplicate canonical claim identity/);
});

test('legacy APIs cannot load unresolved native holds and can resume after canonical completion', async () => {
  const f = fixture(); f.service.acquire('ordinary', 'gah', '#other');
  assert.ok(Array.isArray(JSON.parse(readFileSync(f.path, 'utf8')).leases));
  f.service.reserveNative('central', 'gah', '#1525', f.binding);
  const held = JSON.parse(readFileSync(f.path, 'utf8'));
  assert.equal(held.version, 2);
  assert.equal(held.leases, undefined, 'old ClaimsService rejects this file before granting any lease');
  assert.equal(held.fenced_leases.length, 2);
  await f.service.completeNative('central', 'gah', '#1525', f.binding, async () => f.ack);
  const completed = JSON.parse(readFileSync(f.path, 'utf8'));
  assert.ok(Array.isArray(completed.leases));
  assert.equal(completed.leases[0].node_id, 'ordinary');
});
