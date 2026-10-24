import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { Request, Response, NextFunction } from 'express';
import { framePolicy, forwardedPrefix, gitlabOrigins, webEntry } from './embed.js';

test('framing grants use explicit origins only', () => {
  assert.deepEqual(gitlabOrigins('https://gitlab.example,https://gitlab.other https://gitlab.example * https://evil/path https://user:pass@evil javascript:foo'), ['https://gitlab.example', 'https://gitlab.other']);
});
test('forwarded prefix is safe for HTML and cannot be a URL', () => {
  assert.equal(forwardedPrefix('/parasaur/'), '/parasaur');
  assert.equal(forwardedPrefix('/tools/parasaur'), '/tools/parasaur');
  for (const bad of ['//evil', '/a/../b', '/a"', 'https://evil', '/a,b']) assert.equal(forwardedPrefix(bad), '');
});
test('entry preserves relative build assets and rewrites root assets exactly once', () => {
  const html = webEntry('<html><head><script src="./assets/app.js"></script><link href="/icon.png"></head></html>', '/parasaur', ['https://gitlab.example']);
  assert.match(html, /<base href="\/parasaur\/">/);
  assert.match(html, /src="\.\/assets\/app.js"/);
  assert.match(html, /href="\/parasaur\/icon.png"/);
  assert.match(html, /"gitlabOrigins":\["https:\/\/gitlab.example"\]/);
  assert.doesNotMatch(html, /parasaur\/parasaur/);
});
test('only the embed route is frameable, and missing configuration fails closed', () => {
  const previous = process.env.GAH_GITLAB_ORIGINS;
  process.env.GAH_GITLAB_ORIGINS = 'https://gitlab.example';
  const headersFor = (path: string) => {
    const headers = new Map<string, string>();
    let continued = false;
    framePolicy({ path } as Request, { setHeader: (name: string, value: string) => headers.set(name, value) } as unknown as Response, (() => { continued = true; }) as NextFunction);
    assert.equal(continued, true);
    return headers;
  };
  try {
    const embed = headersFor('/embed');
    assert.equal(embed.get('Content-Security-Policy'), 'frame-ancestors https://gitlab.example');
    assert.equal(embed.has('X-Frame-Options'), false);
    for (const path of ['/', '/api/status', '/embed/other']) {
      const headers = headersFor(path);
      assert.equal(headers.get('Content-Security-Policy'), "frame-ancestors 'none'");
      assert.equal(headers.get('X-Frame-Options'), 'DENY');
    }
    delete process.env.GAH_GITLAB_ORIGINS;
    assert.equal(headersFor('/embed').get('Content-Security-Policy'), "frame-ancestors 'none'");
  } finally {
    if (previous === undefined) delete process.env.GAH_GITLAB_ORIGINS; else process.env.GAH_GITLAB_ORIGINS = previous;
  }
});
