import assert from 'node:assert/strict';
import { test } from 'node:test';
import http from 'node:http';
import net from 'node:net';
import { once } from 'node:events';
import type { AddressInfo } from 'node:net';
import express from 'express';
import rateLimit from 'express-rate-limit';
import { previewProxy, detectDevPort, loopbackPortListening, PreviewTargetRefusedError, PREVIEW_SECRET_PARAM } from './previewProxy.js';
import { authMiddleware } from '../authMiddleware.js';

// Below every common ephemeral range (Linux 32768+, macOS 49152+), so a
// concurrently running test file's outbound sockets can't hold these ports.
const TEST_BASE_PORT = 31_900;
const TEST_MAX_PORT = 31_949;

/** listen(0) on an ephemeral port outside the preview range the tests
 * configure, which `set()` refuses as a target. */
async function listenOutsidePreviewRange(server: http.Server): Promise<number> {
  for (;;) {
    await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
    const port = (server.address() as AddressInfo).port;
    if (port < TEST_BASE_PORT || port > TEST_MAX_PORT) return port;
    await new Promise<void>((done) => server.close(() => done()));
  }
}

/** A fake dev server that records the Host header it was served with. */
async function fakeDevServer(handler: http.RequestListener): Promise<{ server: http.Server; port: number; close: () => Promise<void> }> {
  const server = http.createServer(handler);
  const port = await listenOutsidePreviewRange(server);
  return {
    server,
    port,
    close: () => new Promise<void>((done) => server.close(() => done()))
  };
}

function fetchUrl(url: string, headers: http.OutgoingHttpHeaders = {}): Promise<{ status: number; body: string; headers: http.IncomingHttpHeaders }> {
  return new Promise((resolve, reject) => {
    // agent:false -- a fresh connection per fetch. The global agent pools
    // keep-alive sockets per host:port, and a pooled socket to a preview
    // port whose listener was closed+rebound mid-suite gets reused after
    // the server destroyed it (ECONNRESET). Browsers don't do this.
    http.get(url, { agent: false, headers }, (res) => {
      let body = '';
      res.on('data', (chunk) => { body += chunk; });
      res.on('end', () => resolve({ status: res.statusCode ?? 0, body, headers: res.headers }));
    }).on('error', reject);
  });
}

/** Follows the advertised URL like a browser: trades the URL secret for
 * the access cookie, then fetches `path` on the listener with it. */
async function fetchPreview(url: string, path = '/'): Promise<{ status: number; body: string; headers: http.IncomingHttpHeaders }> {
  const cookie = await previewCookie(url);
  return fetchUrl(new URL(path, url).toString(), { cookie });
}

async function previewCookie(url: string): Promise<string> {
  const exchange = await fetchUrl(url);
  assert.equal(exchange.status, 302, 'the URL secret is exchanged for a cookie');
  const setCookie = exchange.headers['set-cookie']?.[0];
  assert.ok(setCookie, 'access cookie set');
  return setCookie.split(';')[0];
}

/** Sends a raw WebSocket upgrade and resolves with the first chunk that
 * comes back, or '' when the socket closes without one. */
async function rawUpgrade(port: number, extraHeaders: string[] = []): Promise<string> {
  const socket = net.connect(port, '127.0.0.1');
  await once(socket, 'connect');
  socket.write([
    'GET /hmr HTTP/1.1',
    `host: 127.0.0.1:${port}`,
    'connection: Upgrade',
    'upgrade: websocket',
    'sec-websocket-version: 13',
    'sec-websocket-key: dGhlIHNhbXBsZSBub25jZQ==',
    ...extraHeaders,
    '',
    ''
  ].join('\r\n'));
  return new Promise((resolve) => {
    let received = '';
    socket.on('data', (chunk) => {
      received += chunk;
      socket.destroy();
      resolve(received);
    });
    socket.on('close', () => resolve(received));
    socket.on('error', () => resolve(received));
  });
}

test('detectDevPort matches common dev-server output shapes, last match wins', () => {
  assert.equal(detectDevPort('VITE ready\n  Local: http://localhost:5173/'), 5173);
  assert.equal(detectDevPort('listening on 127.0.0.1:3000'), 3000);
  assert.equal(detectDevPort('Server running at http://[::1]:8080'), 8080);
  assert.equal(detectDevPort('Server running on port 4200'), 4200);
  assert.equal(detectDevPort('restarting...\nLocal: http://localhost:3000/\nLocal: http://localhost:3001/'), 3001);
  assert.equal(detectDevPort('nothing here'), null);
  // Sanity bounds: system ports and nonsense are ignored.
  assert.equal(detectDevPort('localhost:80'), null);
});

test('preview proxy serves the dev server through a dedicated port with the Host rewritten to localhost', async () => {
  previewProxy.configure({ basePort: TEST_BASE_PORT, maxPort: TEST_MAX_PORT, advertiseHost: '127.0.0.1', bindHost: '127.0.0.1' });
  const seenHosts: string[] = [];
  const dev = await fakeDevServer((req, res) => {
    seenHosts.push(String(req.headers.host));
    res.writeHead(200, { 'content-type': 'text/html' });
    res.end('<h1>preview</h1>');
  });
  try {
    const preview = await previewProxy.set('p', 's1', dev.port);
    assert.ok(preview.listenPort >= TEST_BASE_PORT && preview.listenPort <= TEST_MAX_PORT, 'listen port from the configured range');
    assert.equal(preview.devPort, dev.port);
    assert.match(preview.url, new RegExp(`^http://127\\.0\\.0\\.1:${preview.listenPort}/\\?${PREVIEW_SECRET_PARAM}=[A-Za-z0-9_-]{43}$`));

    const served = await fetchPreview(preview.url);
    assert.equal(served.status, 200);
    assert.match(served.body, /preview/);
    assert.deepEqual(seenHosts, [`localhost:${dev.port}`], 'Host header rewritten (defeats dev-server allowedHosts checks)');

    // Same dev port set again is idempotent (same listener, no leak).
    const again = await previewProxy.set('p', 's1', dev.port);
    assert.equal(again.listenPort, preview.listenPort);
    assert.equal(again.url, preview.url, 'the access secret is stable for the listener');

    await previewProxy.clear('p', 's1');
    assert.equal(previewProxy.get('p', 's1'), null);
  } finally {
    await previewProxy.clear('p', 's1');
    await dev.close();
  }
});

test('preview proxy re-points a session to a new dev port (stable URL) and reports unreachable targets as 502', async () => {
  previewProxy.configure({ basePort: TEST_BASE_PORT, maxPort: TEST_MAX_PORT, advertiseHost: '127.0.0.1', bindHost: '127.0.0.1' });
  const first = await fakeDevServer((_req, res) => { res.end('one'); });
  const second = await fakeDevServer((_req, res) => { res.end('two'); });
  try {
    const p1 = await previewProxy.set('p', 's2', first.port);
    const p2 = await previewProxy.set('p', 's2', second.port);
    // The listen port is stable across re-points: a dev-server restart on
    // a new port keeps the same preview URL (the proxy resolves the
    // current target per request).
    assert.equal(p2.listenPort, p1.listenPort, 'stable listen port across re-points');
    assert.equal(p2.devPort, second.port);

    const served = await fetchPreview(p2.url);
    assert.match(served.body, /two/);

    // Nothing is listening on this port: the proxy answers 502 with a
    // helpful message instead of hanging.
    const dead = await fakeDevServer(() => { /* never used */ });
    await dead.close();
    const deadPort = dead.port;
    const p3 = await previewProxy.set('p', 's2', deadPort);
    const unreachable = await fetchPreview(p3.url);
    assert.equal(unreachable.status, 502);
    assert.match(unreachable.body, /unreachable/);
  } finally {
    await previewProxy.clear('p', 's2');
    await first.close();
    await second.close();
  }
});

test('preview proxy skips an occupied port and reports the port it actually bound', async () => {
  previewProxy.configure({ basePort: TEST_BASE_PORT, maxPort: TEST_MAX_PORT, advertiseHost: '127.0.0.1', bindHost: '127.0.0.1' });
  const squatter = http.createServer();
  await new Promise<void>((done) => squatter.listen(TEST_BASE_PORT, '127.0.0.1', done));
  const dev = await fakeDevServer((_req, res) => { res.end('dev'); });
  try {
    const preview = await previewProxy.set('p', 'busy', dev.port);
    assert.notEqual(preview.listenPort, TEST_BASE_PORT);
    assert.equal(new URL(preview.url).port, String(preview.listenPort));
    const served = await fetchPreview(preview.url);
    assert.match(served.body, /dev/);
  } finally {
    await previewProxy.clear('p', 'busy');
    await dev.close();
    await new Promise<void>((done) => squatter.close(() => done()));
  }
});

test('a preview never inherits the control plane\'s loopback owner trust', async () => {
  // The proxy connects upstream from loopback with Host rewritten to
  // localhost. Without a forwarding marker, authMiddleware would treat a
  // remote preview visitor as the local owner.
  previewProxy.configure({ basePort: TEST_BASE_PORT, maxPort: TEST_MAX_PORT, advertiseHost: '127.0.0.1', bindHost: '127.0.0.1', protectedPorts: [] });
  const app = express();
  app.use(rateLimit({ windowMs: 60_000, limit: 200, validate: false }), authMiddleware);
  app.get('/api/whoami', (_req, res) => { res.json({ principal: res.locals.authPrincipal }); });
  const api = http.createServer(app);
  const apiPort = await listenOutsidePreviewRange(api);
  try {
    const direct = await fetchUrl(`http://127.0.0.1:${apiPort}/api/whoami`);
    assert.equal(direct.status, 200, 'a direct loopback request is the local owner');
    const preview = await previewProxy.set('p', 'trust', apiPort);
    const proxied = await fetchPreview(preview.url, '/api/whoami');
    assert.notEqual(proxied.status, 200, 'a request through the preview port must not be the owner');
  } finally {
    await previewProxy.clear('p', 'trust');
    await new Promise<void>((done) => api.close(() => done()));
  }
});

test('a preview refuses the control-plane port and its own listener range', async () => {
  previewProxy.configure({ basePort: TEST_BASE_PORT, maxPort: TEST_MAX_PORT, advertiseHost: '127.0.0.1', protectedPorts: [3773] });
  await assert.rejects(previewProxy.set('p', 'cp', 3773), PreviewTargetRefusedError);
  await assert.rejects(previewProxy.set('p', 'loop', TEST_BASE_PORT + 10), PreviewTargetRefusedError);
  assert.equal(previewProxy.get('p', 'cp'), null);
});

test('a preview bound to one interface advertises that interface', async () => {
  // With HOST=127.0.0.1 and no advertiseHost, a Tailscale address would be
  // unreachable: the listener only accepts connections on the bind address.
  previewProxy.configure({ basePort: TEST_BASE_PORT, maxPort: TEST_MAX_PORT, advertiseHost: undefined, bindHost: '127.0.0.1', protectedPorts: [] });
  const dev = await fakeDevServer((_req, res) => { res.end('ok'); });
  try {
    const preview = await previewProxy.set('p', 'bound', dev.port);
    assert.ok(preview.url.startsWith('http://127.0.0.1:'), preview.url);
    assert.equal((await fetchPreview(preview.url)).body, 'ok');
  } finally {
    await previewProxy.clear('p', 'bound');
    await dev.close();
    previewProxy.configure({ advertiseHost: '127.0.0.1' });
  }
});

test('a preview without its access secret is refused before upstream is contacted', async () => {
  previewProxy.configure({ basePort: TEST_BASE_PORT, maxPort: TEST_MAX_PORT, advertiseHost: '127.0.0.1', bindHost: '127.0.0.1', protectedPorts: [] });
  let upstreamConnections = 0;
  const dev = await fakeDevServer((_req, res) => { res.end('secret app'); });
  dev.server.on('connection', () => { upstreamConnections++; });
  dev.server.on('upgrade', (_req, socket) => { socket.end('HTTP/1.1 101 Switching Protocols\r\n\r\n'); });
  const other = await fakeDevServer((_req, res) => { res.end('other'); });
  try {
    const preview = await previewProxy.set('p', 'deny', dev.port);
    const otherPreview = await previewProxy.set('p', 'deny-other', other.port);
    const origin = new URL(preview.url).origin;

    assert.equal((await fetchUrl(`${origin}/`)).status, 403);
    assert.equal((await fetchUrl(`${origin}/?${PREVIEW_SECRET_PARAM}=wrong`)).status, 403);
    assert.equal((await fetchUrl(`${origin}/`, { cookie: `__gah_preview_${preview.listenPort}=wrong` })).status, 403);
    // Another listener's cookie reaches this port too, but never opens it.
    const otherCookie = await previewCookie(otherPreview.url);
    assert.equal((await fetchUrl(`${origin}/`, { cookie: otherCookie })).status, 403);

    assert.equal(await rawUpgrade(preview.listenPort), '', 'an upgrade without the secret is dropped');
    assert.equal(await rawUpgrade(preview.listenPort, [`cookie: __gah_preview_${preview.listenPort}=wrong`]), '');
    assert.equal(upstreamConnections, 0, 'refused traffic never reaches the dev server');

    const cookie = await previewCookie(preview.url);
    assert.match(await rawUpgrade(preview.listenPort, [`cookie: ${cookie}`]), /^HTTP\/1\.1 101/, 'an authenticated upgrade is spliced');
    assert.equal(upstreamConnections, 1);
  } finally {
    await previewProxy.clear('p', 'deny');
    await previewProxy.clear('p', 'deny-other');
    await dev.close();
    await other.close();
  }
});

test('the URL secret is exchanged for a listener-scoped cookie and stripped before proxying', async () => {
  previewProxy.configure({ basePort: TEST_BASE_PORT, maxPort: TEST_MAX_PORT, advertiseHost: '127.0.0.1', bindHost: '127.0.0.1', protectedPorts: [] });
  const seen: { url: string | undefined; cookie: string | undefined }[] = [];
  const dev = await fakeDevServer((req, res) => {
    seen.push({ url: req.url, cookie: req.headers.cookie });
    res.end('app');
  });
  try {
    const preview = await previewProxy.set('p', 'cookie', dev.port);
    const secret = new URL(preview.url).searchParams.get(PREVIEW_SECRET_PARAM);
    const origin = new URL(preview.url).origin;

    const exchange = await fetchUrl(`${origin}/page?a=1&${PREVIEW_SECRET_PARAM}=${secret}`);
    assert.equal(exchange.status, 302);
    assert.equal(exchange.headers.location, '/page?a=1', 'the secret is stripped, other params kept');
    const setCookie = exchange.headers['set-cookie']?.[0] ?? '';
    assert.match(setCookie, new RegExp(`^__gah_preview_${preview.listenPort}=${secret}; `));
    assert.match(setCookie, /HttpOnly/);
    assert.match(setCookie, /SameSite=Lax/);
    assert.equal(seen.length, 0, 'the exchange itself is not proxied');

    const protocolRelative = await fetchUrl(`${origin}//elsewhere.invalid/x?${PREVIEW_SECRET_PARAM}=${secret}`);
    assert.equal(protocolRelative.headers.location, '/', 'never a protocol-relative redirect');

    const accessCookie = setCookie.split(';')[0];
    const served = await fetchUrl(`${origin}/page?a=1`, { cookie: `theme=dark; ${accessCookie}; __gah_preview_1=x` });
    assert.equal(served.status, 200);
    assert.equal(served.body, 'app');
    assert.deepEqual(seen, [{ url: '/page?a=1', cookie: 'theme=dark' }], 'preview cookies never reach the dev server');
  } finally {
    await previewProxy.clear('p', 'cookie');
    await dev.close();
  }
});

test('auto-detection probes the loopback: a port with no listener is not adopted', async () => {
  const dev = await fakeDevServer((_req, res) => { res.end('ok'); });
  const dead = await fakeDevServer(() => { /* never used */ });
  await dead.close();
  try {
    assert.equal(await loopbackPortListening(dev.port), true);
    assert.equal(await loopbackPortListening(dead.port), false);
  } finally {
    await dev.close();
  }
});

test('a preview refuses the memory gateway port', async () => {
  const saved = { url: process.env.TDAI_GATEWAY_URL, settings: process.env.GAH_GATEWAY_SETTINGS_PATH };
  process.env.GAH_GATEWAY_SETTINGS_PATH = '/nonexistent/gateway-settings.json';
  previewProxy.configure({ basePort: TEST_BASE_PORT, maxPort: TEST_MAX_PORT, advertiseHost: '127.0.0.1', protectedPorts: [] });
  try {
    delete process.env.TDAI_GATEWAY_URL;
    await assert.rejects(previewProxy.set('p', 'gw', 8420), PreviewTargetRefusedError);
    process.env.TDAI_GATEWAY_URL = 'http://127.0.0.1:18420';
    await assert.rejects(previewProxy.set('p', 'gw', 18420), PreviewTargetRefusedError);
    assert.equal(previewProxy.get('p', 'gw'), null);
  } finally {
    if (saved.url === undefined) delete process.env.TDAI_GATEWAY_URL; else process.env.TDAI_GATEWAY_URL = saved.url;
    if (saved.settings === undefined) delete process.env.GAH_GATEWAY_SETTINGS_PATH; else process.env.GAH_GATEWAY_SETTINGS_PATH = saved.settings;
  }
});
