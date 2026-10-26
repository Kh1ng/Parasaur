import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import http from 'node:http';
import express from 'express';
import { createServer } from '../../server.js';
import {
  appHtmlHandler,
  bootstrapScript,
  embedHtmlHandler,
  framePolicyHeader,
  gitLabFrameOrigins,
  isApiRequest,
  publicPathPrefix
} from './embedView.js';

test('frame origins parse GAH_GITLAB_ORIGIN, drop invalid entries, and fail closed when unset', () => {
  assert.deepEqual(gitLabFrameOrigins(undefined), []);
  assert.deepEqual(gitLabFrameOrigins(''), []);
  assert.deepEqual(gitLabFrameOrigins('https://gitlab.example.com'), ['https://gitlab.example.com']);
  // Whitespace and comma both separate; duplicates collapse.
  assert.deepEqual(gitLabFrameOrigins('https://gitlab.example.com, https://gitlab.example.com https://gitlab.two.example'),
    ['https://gitlab.example.com', 'https://gitlab.two.example']);
  // Entries with a path, credentials, or a non-http scheme never authorize framing.
  assert.deepEqual(gitLabFrameOrigins('https://gitlab.example.com/prefix https://user@gitlab.example.com ftp://gitlab.example.com javascript:alert(1) notaurl'),
    []);
  // Ports survive; the list stays bounded.
  assert.deepEqual(gitLabFrameOrigins('http://gitlab.internal:8080'), ['http://gitlab.internal:8080']);
  assert.equal(gitLabFrameOrigins(Array.from({ length: 12 }, (_, index) => `https://gl${index}.example`).join(' ')).length, 8);
});

test('the frame policy header names every configured origin and fails closed', () => {
  assert.equal(framePolicyHeader([]), "frame-ancestors 'none'");
  assert.equal(framePolicyHeader(['https://gitlab.example.com']), 'frame-ancestors https://gitlab.example.com');
  assert.equal(framePolicyHeader(['https://one.example', 'https://two.example']), 'frame-ancestors https://one.example https://two.example');
});

test('the public path prefix accepts plain segments and rejects origin smuggling', () => {
  assert.equal(publicPathPrefix(undefined), '');
  assert.equal(publicPathPrefix(''), '');
  assert.equal(publicPathPrefix('/parasaur'), '/parasaur');
  assert.equal(publicPathPrefix('/parasaur/'), '/parasaur');
  assert.equal(publicPathPrefix(' /parasaur '), '/parasaur');
  assert.equal(publicPathPrefix('/a/b'), '/a/b');
  for (const malformed of ['//evil.example', '/..', '/../parasaur', '/parasaur/../x', 'parasaur', '/parasaur?', '/parasaur#', `/${'a'.repeat(129)}`, '/parasaur/%20', 'https://evil.example']) {
    assert.equal(publicPathPrefix(malformed), '', `rejects ${malformed}`);
  }
});

test('the bootstrap script injects the base path, origins, and escapes markup', () => {
  const plain = bootstrapScript('/parasaur', ['https://gitlab.example.com']);
  assert.ok(plain.includes('<base href="/parasaur/">'));
  assert.ok(plain.includes('"basePath":"/parasaur"'));
  assert.ok(plain.includes('"gitLabOrigins":["https://gitlab.example.com"]'));
  assert.equal(bootstrapScript('', []).includes('<base'), false);
  // A `<` in configured values cannot close the script tag early.
  assert.equal(bootstrapScript('/parasaur', ['https://gitlab.example.com</script><script>alert(1)</script>']).includes('</script><script>alert'), false);
});

test('the API boundary never falls through to the SPA document', () => {
  for (const path of ['/api', '/api/status', '/api/integrations/gitlab/events']) assert.equal(isApiRequest(path), true);
  for (const path of ['/', '/embed', '/parasaur/embed', '/health']) assert.equal(isApiRequest(path), false);
});

async function serve(app: express.Express) {
  const server = http.createServer(app);
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address(); assert.ok(address && typeof address !== 'string');
  return { base: `http://127.0.0.1:${address.port}`, close: () => new Promise<void>(resolve => server.close(() => resolve())) };
}

test('embed route is frameable only by the configured GitLab origin; the rest of the app stays non-frameable', async () => {
  const webRoot = mkdtempSync(join(tmpdir(), 'gah-embed-web-'));
  writeFileSync(join(webRoot, 'index.html'), '<!DOCTYPE html><html><head><title>Parasaur</title></head><body><div id="root"></div></body></html>');
  writeFileSync(join(webRoot, 'asset.txt'), 'shell');
  const saved = { GAH_WEB_ROOT: process.env.GAH_WEB_ROOT, GAH_GITLAB_ORIGIN: process.env.GAH_GITLAB_ORIGIN };
  process.env.GAH_WEB_ROOT = webRoot;
  process.env.GAH_GITLAB_ORIGIN = 'https://gitlab.example.com https://misconfigured.example.com/rejected';
  const app = createServer({});
  const server = await serve(app);
  try {
    const embed = await fetch(`${server.base}/embed?gitlab=https%3A%2F%2Fgitlab.example.com&url=&project=&kind=&iid=&ref=&path=`);
    assert.equal(embed.status, 200);
    assert.equal(embed.headers.get('content-security-policy'), 'frame-ancestors https://gitlab.example.com');
    assert.equal(embed.headers.get('x-frame-options'), null);
    assert.match(await embed.text(), /__GAH_BOOTSTRAP__/);

    const standalone = await fetch(`${server.base}/?gitlab=https%3A%2F%2Fgitlab.example.com`);
    assert.equal(standalone.status, 200);
    assert.equal(standalone.headers.get('x-frame-options'), 'DENY');
    assert.equal(standalone.headers.get('content-security-policy'), null);

    // Static assets keep flowing through the same web root.
    assert.equal(await (await fetch(`${server.base}/asset.txt`)).text(), 'shell');
    // API paths never fall through to the SPA document.
    const api = await fetch(`${server.base}/api/does-not-exist`);
    assert.equal(api.status, 404);
    assert.equal((await api.json() as { error?: string }).error, 'Not Found');
  } finally {
    await server.close();
    for (const [key, value] of Object.entries(saved)) { if (value === undefined) delete process.env[key]; else process.env[key] = value; }
    rmSync(webRoot, { recursive: true, force: true });
  }
});

test('with GAH_GITLAB_ORIGIN unset the embed route fails closed: not frameable', async () => {
  const webRoot = mkdtempSync(join(tmpdir(), 'gah-embed-web-'));
  writeFileSync(join(webRoot, 'index.html'), '<!DOCTYPE html><html><head></head><body></body></html>');
  const saved = { GAH_WEB_ROOT: process.env.GAH_WEB_ROOT, GAH_GITLAB_ORIGIN: process.env.GAH_GITLAB_ORIGIN };
  process.env.GAH_WEB_ROOT = webRoot;
  delete process.env.GAH_GITLAB_ORIGIN;
  const app = createServer({});
  const server = await serve(app);
  try {
    const embed = await fetch(`${server.base}/embed`);
    assert.equal(embed.status, 200);
    assert.equal(embed.headers.get('content-security-policy'), "frame-ancestors 'none'");
    assert.equal(embed.headers.get('x-frame-options'), null);
    assert.equal((await embed.text()).includes('frame-ancestors'), false);
  } finally {
    await server.close();
    for (const [key, value] of Object.entries(saved)) { if (value === undefined) delete process.env[key]; else process.env[key] = value; }
    rmSync(webRoot, { recursive: true, force: true });
  }
});

test('X-Forwarded-Prefix injects the base path into served documents', async () => {
  const webRoot = mkdtempSync(join(tmpdir(), 'gah-embed-web-'));
  writeFileSync(join(webRoot, 'index.html'), '<!DOCTYPE html><html><head><title>Parasaur</title></head><body></body></html>');
  const app = express();
  app.get('/embed', embedHtmlHandler({ webRoot }));
  app.get('*', appHtmlHandler({ webRoot }));
  const server = await serve(app);
  try {
    const embed = await fetch(`${server.base}/embed?project=team%2Fproject`, { headers: { 'X-Forwarded-Prefix': '/parasaur' } });
    const document = await embed.text();
    assert.match(document, /<base href="\/parasaur\/">/);
    assert.match(document, /"basePath":"\/parasaur"/);
    const root = await fetch(`${server.base}/`, { headers: { 'X-Forwarded-Prefix': '/parasaur' } });
    assert.match(await root.text(), /"basePath":"\/parasaur"/);
    // A prefix that tries to smuggle another origin injects nothing.
    const smuggled = await fetch(`${server.base}/`, { headers: { 'X-Forwarded-Prefix': '//evil.example' } });
    assert.equal((await smuggled.text()).includes('evil.example'), false);
  } finally {
    await server.close();
    rmSync(webRoot, { recursive: true, force: true });
  }
});

test('non-HTML requests fall through the embed route to the JSON 404', async () => {
  const webRoot = mkdtempSync(join(tmpdir(), 'gah-embed-web-'));
  writeFileSync(join(webRoot, 'index.html'), '<!DOCTYPE html><html><head></head><body></body></html>');
  const app = express();
  app.get('/embed', embedHtmlHandler({ webRoot }));
  app.use((_req, res) => res.status(404).json({ error: 'Not Found' }));
  const server = await serve(app);
  try {
    const json = await fetch(`${server.base}/embed`, { headers: { Accept: 'application/json' } });
    assert.equal(json.status, 404);
    assert.equal((await json.json() as { error?: string }).error, 'Not Found');
  } finally {
    await server.close();
    rmSync(webRoot, { recursive: true, force: true });
  }
});
