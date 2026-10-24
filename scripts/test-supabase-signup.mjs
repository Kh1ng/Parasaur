// Called by test-supabase-migrations.sh with disposable local CLI status on stdin.
import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { readFileSync } from 'node:fs';

const { API_URL, ANON_KEY } = JSON.parse(readFileSync(0, 'utf8'));
const url = new URL(API_URL);
assert.equal(url.protocol, 'http:');
assert.ok(['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname), 'signup test must stay local');
assert.ok(ANON_KEY, 'local anon key is required');

const response = await fetch(new URL('/auth/v1/signup', url), {
  method: 'POST',
  redirect: 'error',
  signal: AbortSignal.timeout(10_000),
  headers: { apikey: ANON_KEY, 'Content-Type': 'application/json' },
  body: JSON.stringify({ email: 'closed-beta-unknown@example.test', password: randomUUID() }),
});
assert.ok(response.status >= 400 && response.status < 500, `signup must return 4xx, got ${response.status}`);
const error = await response.json();
assert.equal(error.error_code, 'signup_disabled');
assert.equal(error.msg, 'Signups not allowed for this instance');
console.log(`Unknown-email signup rejected: HTTP ${response.status}, ${error.error_code}, ${error.msg}`);
