import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { AgyQuotaScopes } from './agyQuotaScopes.js';

test('AGY scopes survive reconstruction and evict accounts beyond the bound', t => {
  const root = mkdtempSync(join(tmpdir(), 'gah-agy-scopes-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const path = join(root, 'scopes.json');
  const store = new AgyQuotaScopes(path);
  const scope = [{ quotaPool: 'provider-weekly', window: 'weekly' }];
  for (let index = 0; index < 65; index++) store.set(`account-${index}`, scope);
  assert.equal(store.get('account-0'), undefined);
  const restarted = new AgyQuotaScopes(path);
  assert.equal(restarted.get('account-0'), undefined);
  assert.deepEqual(restarted.get('account-64'), scope);
  assert.equal(JSON.parse(readFileSync(path, 'utf8')).length, 64);
});
