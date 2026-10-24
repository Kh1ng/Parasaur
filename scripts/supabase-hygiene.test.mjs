import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { lstatSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

// Hosted project refs must not appear in tracked text files (#1601).
// The supabase/ tree also excludes service-role key material.
const hostedRef = /[a-z]{20}\.supabase\.co/;
const keyMaterial = [
  // Service-role keys are JWTs; new-style Supabase API keys start sb_secret_.
  /eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/,
  /sb_secret_[A-Za-z0-9]{8,}/,
  /service[_-]role[^;\n]{0,32}[=:]\s*["']?[A-Za-z0-9_-]{16,}/i,
];

const root = fileURLToPath(new URL('..', import.meta.url));
const tracked = execFileSync('git', ['ls-files', '-z'], { cwd: root, encoding: 'utf8' })
  .split('\0')
  .filter(Boolean);

test('tracked text files carry no hosted project refs', () => {
  for (const path of tracked) {
    assert.ok(!hostedRef.test(path), `${path} names a hosted project`);
    // Submodules are tracked as directories; binaries have NUL bytes or invalid UTF-8.
    if (!lstatSync(join(root, path)).isFile()) continue;
    const bytes = readFileSync(join(root, path));
    if (bytes.includes(0)) continue;
    let content;
    try { content = new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
    catch { continue; }
    assert.ok(!hostedRef.test(content), `${path} contains a hosted project ref URL`);
  }
});

test('tracked supabase files carry no service-role key material', () => {
  for (const path of tracked.filter(path => path.startsWith('supabase/'))) {
    const content = readFileSync(join(root, path), 'utf8');
    for (const pattern of keyMaterial) {
      assert.ok(!pattern.test(content), `${path} contains service-role key material`);
    }
  }
});

test('linked CLI state stays out of the repository', () => {
  assert.deepEqual(tracked.filter(path => /^supabase\/\.(temp|branches)\//.test(path)), []);
});

test('config.toml keeps public sign-ups disabled in every auth section', () => {
  const config = readFileSync(join(root, 'supabase/config.toml'), 'utf8');
  for (const section of ['[auth]', '[auth.email]', '[auth.sms]']) {
    const body = config.split(section)[1]?.split(/\n\[/)[0] ?? '';
    assert.match(body, /^enable_signup\s*=\s*false$/m, `${section} must set enable_signup = false`);
  }
});
