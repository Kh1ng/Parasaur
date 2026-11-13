import assert from 'node:assert/strict';
import { test, type TestContext } from 'node:test';
import { LinkRefused, linkEnabled, linkTo, type LinkRegistration } from './workerLink.js';

const SETTINGS = ['GAH_WORKER_LINKS', 'GAH_ALLOW_INSECURE_HTTP', 'LINK_TEST_CREDENTIAL'] as const;

/** Each test states the whole link environment and leaves the process as it found it. */
function environment(t: TestContext, values: Partial<Record<typeof SETTINGS[number], string>>) {
  const saved = SETTINGS.map(name => [name, process.env[name]] as const);
  for (const name of SETTINGS) delete process.env[name];
  Object.assign(process.env, values);
  t.after(() => { for (const [name, value] of saved) { if (value === undefined) delete process.env[name]; else process.env[name] = value; } });
}

const worker = (advertised_url: string, transport_mode: LinkRegistration['transport_mode'] = 'trusted_lan'): LinkRegistration =>
  ({ advertised_url, transport_mode, secret_ref: 'env:LINK_TEST_CREDENTIAL' });

test('the kind of a link comes from the address alone', t => {
  environment(t, { GAH_WORKER_LINKS: 'tailnet,lan' });
  const kinds = Object.fromEntries([
    'http://127.0.0.1:3774', 'http://localhost:3774', 'http://[::1]:3774', 'https://localhost:3774',
    'https://worker.example.com', 'https://mac.tail82695.ts.net:4774',
    'http://100.64.0.1:3774', 'http://100.98.69.105:3774', 'http://100.127.255.254:3774',
    'http://[fd7a:115c:a1e0::7234:6151]:3774', 'http://mac.tail82695.ts.net:3774',
    'http://100.63.255.255:3774', 'http://100.128.0.1:3774', 'http://192.168.5.15:3774', 'http://127.example.com:3774',
  ].map(address => [address, linkTo(worker(address)).kind]));
  assert.deepEqual(kinds, {
    'http://127.0.0.1:3774': 'loopback', 'http://localhost:3774': 'loopback', 'http://[::1]:3774': 'loopback', 'https://localhost:3774': 'loopback',
    'https://worker.example.com': 'https', 'https://mac.tail82695.ts.net:4774': 'https',
    'http://100.64.0.1:3774': 'tailnet', 'http://100.98.69.105:3774': 'tailnet', 'http://100.127.255.254:3774': 'tailnet',
    'http://[fd7a:115c:a1e0::7234:6151]:3774': 'tailnet', 'http://mac.tail82695.ts.net:3774': 'tailnet',
    'http://100.63.255.255:3774': 'lan', 'http://100.128.0.1:3774': 'lan', 'http://192.168.5.15:3774': 'lan', 'http://127.example.com:3774': 'lan',
  });
});

test('a central sends plain HTTP only over the kinds its operator enabled', t => {
  const tailnet = worker('http://100.98.69.105:3774'), lan = worker('http://192.168.5.15:3774');
  environment(t, {});
  assert.throws(() => linkTo(tailnet), (error: unknown) => error instanceof LinkRefused
    && /http:\/\/100\.98\.69\.105:3774/.test(error.message) && /'tailnet' to GAH_WORKER_LINKS/.test(error.message));
  assert.throws(() => linkTo(lan), (error: unknown) => error instanceof LinkRefused && /'lan' to GAH_WORKER_LINKS/.test(error.message));
  assert.equal(linkTo(worker('https://worker.example.com')).kind, 'https');
  assert.equal(linkTo(worker('http://127.0.0.1:3774')).kind, 'loopback');

  process.env.GAH_WORKER_LINKS = 'tailnet';
  assert.equal(linkTo(tailnet).kind, 'tailnet');
  assert.throws(() => linkTo(lan), LinkRefused, 'a tailnet-only central still refuses an ordinary network address');
  assert.deepEqual([linkEnabled('tailnet'), linkEnabled('lan')], [true, false]);

  process.env.GAH_WORKER_LINKS = 'lan, tailnet';
  assert.equal(linkTo(lan).kind, 'lan');

  delete process.env.GAH_WORKER_LINKS;
  process.env.GAH_ALLOW_INSECURE_HTTP = '1';
  assert.deepEqual([linkTo(tailnet).kind, linkTo(lan).kind], ['tailnet', 'lan'], 'the older switch keeps enabling both');
});

test('a registration whose claimed mode contradicts its address is refused', t => {
  environment(t, { GAH_WORKER_LINKS: 'tailnet,lan' });
  assert.throws(() => linkTo(worker('http://100.98.69.105:3774', 'loopback')), /cannot use loopback transport mode/);
  assert.throws(() => linkTo(worker('http://192.168.5.15:3774', 'authenticated_remote')), /must use TLS/);
  for (const address of ['not a url', 'ftp://192.168.5.15', 'ws://192.168.5.15:3774', 'http://user:secret@192.168.5.15:3774']) {
    assert.throws(() => linkTo(worker(address)), LinkRefused, address);
  }
});

test('the worker credential goes on every link except a local worker that never asked for one', t => {
  environment(t, { GAH_WORKER_LINKS: 'tailnet', LINK_TEST_CREDENTIAL: 'link-credential' });
  assert.equal(linkTo(worker('http://100.98.69.105:3774')).headers().Authorization, 'Bearer link-credential');
  assert.equal(linkTo(worker('https://worker.example.com', 'authenticated_remote')).headers().Authorization, 'Bearer link-credential');
  assert.equal(linkTo(worker('http://127.0.0.1:3774', 'authenticated_remote')).headers().Authorization, 'Bearer link-credential');
  assert.equal(linkTo(worker('http://127.0.0.1:3774', 'loopback')).headers().Authorization, undefined);
  assert.equal(linkTo(worker('http://127.0.0.1:3774', 'trusted_lan')).headers().Authorization, undefined);

  delete process.env.LINK_TEST_CREDENTIAL;
  const unreadable = linkTo(worker('http://100.98.69.105:3774'));
  assert.throws(() => unreadable.headers(), /LINK_TEST_CREDENTIAL is not set/);
  assert.doesNotThrow(() => linkTo(worker('http://127.0.0.1:3774', 'loopback')).headers());
  process.env.LINK_TEST_CREDENTIAL = 'bad\r\nInjected: 1';
  assert.throws(() => unreadable.headers(), /malformed/);
});

test('a request stays on the worker address, carries the credential and refuses redirects', async t => {
  environment(t, { GAH_WORKER_LINKS: 'tailnet', LINK_TEST_CREDENTIAL: 'link-credential' });
  const sent: { url: string; init: RequestInit }[] = [];
  const via = (async (url: URL, init: RequestInit) => { sent.push({ url: String(url), init }); return new Response('{}'); }) as unknown as typeof fetch;
  const link = linkTo(worker('http://100.98.69.105:3774/ignored?x=1'));
  assert.equal(link.origin, 'http://100.98.69.105:3774');
  await link.fetch('/api/status?profile=p', { method: 'POST', headers: { 'Content-Type': 'application/json' }, redirect: 'follow' } as never, via);
  assert.equal(sent[0].url, 'http://100.98.69.105:3774/api/status?profile=p');
  assert.equal(sent[0].init.redirect, 'error');
  assert.equal(sent[0].init.method, 'POST');
  const headers = sent[0].init.headers as Record<string, string>;
  assert.equal(headers.Authorization, 'Bearer link-credential');
  assert.equal(headers['Content-Type'], 'application/json');
  await assert.rejects(link.fetch('//elsewhere.example.com/api/status', {}, via), LinkRefused);
  await assert.rejects(link.fetch('https://elsewhere.example.com/api/status', {}, via), LinkRefused);
  assert.equal(sent.length, 1, 'a path that leaves the worker is never sent');
});
