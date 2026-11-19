import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';
import type { RegisteredNode, RunningWorker } from '@git-agent-harness/contracts';
import { cancellationTarget, cancelFactoryDispatch } from './factoryCancellation.js';

const worker = (run_id: string, node_id: string): RunningWorker =>
  ({ work_id: 'TICKET-001', run_id, profile: 'project', node_id, state: 'running', cancellable:true } as RunningWorker);
const dependencies = {
  localId: 'central', central: true,
  local: async (work_id: string, profile = '', run_id = '') => ({ work_id, profile, run_id }),
  list: async () => [worker('one', 'central')],
  registered: (_id: string): RegisteredNode | undefined => undefined,
};

test('work-only requests resolve ticket aliases and pin exactly one running job', async () => {
  assert.deepEqual(await cancelFactoryDispatch({work_id:'#1'}, dependencies),
    {work_id:'TICKET-001',profile:'project',run_id:'one'});
  await assert.rejects(cancelFactoryDispatch({work_id:'1'}, {...dependencies,
    list: async () => [worker('one','central'),worker('two','worker')]}), /Several dispatches/);
  await assert.rejects(cancelFactoryDispatch({work_id:'1'}, {...dependencies, list: async () => [{...worker('one','central'),cancellable:false}]}), /No matching/);
  await assert.rejects(cancelFactoryDispatch({work_id:'1',run_id:'absent'}, dependencies), /No matching/);
});

test('the node selector cannot specify a destination URL or omit a run', async () => {
  assert.equal(cancellationTarget({work_id:'1', node_id:'worker'}), undefined);
  assert.equal(cancellationTarget({work_id:'1', url:'https://unregistered.invalid'}), undefined);
  assert.equal(cancellationTarget({work_id:''}), undefined);
  assert.equal(cancellationTarget({work_id:'bad\nfield'}), undefined);
  const target = {work_id:'1', profile:'project', run_id:'one', node_id:'worker'};
  assert.deepEqual(cancellationTarget(target),target);
  await assert.rejects(cancelFactoryDispatch(target, dependencies), /unavailable/);
  await assert.rejects(cancelFactoryDispatch(target, {...dependencies,central:false}), /unavailable/);
});

test('worker relay sends an authenticated mutation key and verifies the returned run', async t => {
  const target = {work_id:'TICKET-001', profile:'project', run_id:'one', node_id:'worker'};
  let mismatch = false;
  let observed: unknown;
  let key: string | string[] | undefined;
  const server = createServer(async (req,res) => {
    const body: Buffer[] = [];
    for await (const chunk of req) body.push(chunk);
    observed = [req.method,req.url,JSON.parse(Buffer.concat(body).toString())];
    key = req.headers['idempotency-key'];
    res.writeHead(202,{'Content-Type':'application/json'});
    res.end(JSON.stringify({...target,run_id:mismatch?'two':'one'}));
  });
  await new Promise<void>(resolve => server.listen(0,'127.0.0.1',resolve));
  t.after(() => { server.closeAllConnections(); server.close(); });
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const registered = () => ({node_id:'worker', profiles:['project'], advertised_url:'http://127.0.0.1:'+address.port,
    transport_mode:'loopback'} as RegisteredNode);
  const reply = await cancelFactoryDispatch(target, {...dependencies,registered});
  assert.equal(reply.run_id, 'one');
  assert.deepEqual(observed,['POST','/api/dispatch/cancel',target]);
  assert.match(String(key), /^[A-Za-z0-9_-]{16,128}$/);
  mismatch = true;
  await assert.rejects(cancelFactoryDispatch(target, {...dependencies,registered}), /different dispatch/);
});